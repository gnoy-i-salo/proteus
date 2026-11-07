import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { CliEvent } from './cli';
import type { CliWireRequest } from './cli-wire-codec';
import { cliProviders } from './cli';

const native = vi.hoisted(() => ({
  invoke:
    vi.fn<(command: string, args: { request?: CliWireRequest; id?: string }) => Promise<unknown>>(),
  listen:
    vi.fn<(name: string, receive: (event: { payload: unknown }) => void) => Promise<() => void>>(),
  unlisten: vi.fn(),
  receive: undefined as ((event: { payload: unknown }) => void) | undefined,
}));
vi.mock('@tauri-apps/api/core', () => ({ invoke: native.invoke }));
vi.mock('@tauri-apps/api/event', () => ({ listen: native.listen }));
vi.mock('./window-controls', () => ({ isDesktop: () => true }));

beforeEach(() => {
  native.receive = undefined;
  native.listen.mockImplementation((_name, receive) => {
    native.receive = receive;
    return Promise.resolve(native.unlisten);
  });
});

describe('failed native calls retain public traces', () => {
  it('bounds partial UTF-8 text without splitting a character', async () => {
    native.invoke.mockImplementation((_command, args) => {
      native.receive?.({
        payload: {
          id: args.request!.id,
          kind: 'reasoning',
          elapsedMs: 30,
          reasoning: { id: 'r', kind: 'thinking', text: 'я'.repeat(30000) },
        },
      });
      return Promise.reject(new Error('Stopped'));
    });
    const failure = await cliProviders(() => {})
      .kimi.run({
        task: 'Task',
        systemPrompt: 'PERSON',
        effort: 'medium',
      })
      .catch((error: unknown) => error);
    expect(failure).toMatchObject({
      trace: {
        reasoningTruncated: true,
        reasoning: [{ id: 'r', kind: 'thinking', text: 'я'.repeat(24576) }],
      },
    });
  });
  it('keeps latest public snapshots and elapsed time when cancellation rejects the CLI call', async () => {
    const controller = new AbortController();
    const seen: CliEvent[] = [];
    native.invoke.mockImplementation((command, args) => {
      if (command === 'cancel_cli') return Promise.resolve();
      const id = args.request!.id;
      for (const [elapsed, text] of [
        [100, 'First summary'],
        [150, 'Updated public summary'],
      ] as const)
        native.receive?.({
          payload: {
            id,
            kind: 'reasoning',
            elapsedMs: elapsed,
            reasoning: { id: 'r', kind: 'summary', text },
          },
        });
      native.receive?.({
        payload: {
          id: 'unrelated-run',
          kind: 'reasoning',
          elapsedMs: 900,
          reasoning: { id: 'foreign', kind: 'summary', text: 'Must be ignored' },
        },
      });
      controller.abort();
      return Promise.reject(new Error('CLI cancelled'));
    });
    const failure = await cliProviders((event) => seen.push(event))
      .codex.run(
        {
          task: 'Task',
          systemPrompt: 'PERSON',
          effort: 'high',
          model: 'chosen-alias',
        },
        controller.signal,
      )
      .catch((error: unknown) => error);
    expect(failure).toMatchObject({
      name: 'IntelligenceRunError',
      trace: {
        provider: 'codex',
        wallTimeMs: 150,
        reasoningMs: 150,
        activeReasoningMs: null,
        requestedModel: 'chosen-alias',
        resolvedModel: null,
        requestedEffort: 'high',
        appliedEffort: null,
        cliVersion: null,
        reasoning: [{ id: 'r', kind: 'summary', text: 'Updated public summary' }],
      },
    });
    expect(seen).toHaveLength(2);
    expect(native.unlisten).toHaveBeenCalledOnce();
    expect(native.invoke).toHaveBeenCalledWith('cancel_cli', expect.anything());
  });

  it('does not replace a completed public trace with an empty one when abort arrives after the native answer', async () => {
    const controller = new AbortController();
    native.invoke.mockImplementation((command) => {
      if (command === 'cancel_cli') return Promise.resolve();
      controller.abort();
      return Promise.resolve({
        answer: 'Not accepted after cancellation',
        trace: {
          provider: 'kimi',
          wallTimeMs: 250,
          reasoningMs: 250,
          terminalMs: 0,
          summary: 'Returned',
          reasoning: [{ id: 'thinking', kind: 'thinking', text: 'Public thought' }],
        },
      });
    });
    const failure = await cliProviders(() => {})
      .kimi.run(
        {
          task: 'Task',
          systemPrompt: 'PERSON',
          effort: 'medium',
        },
        controller.signal,
      )
      .catch((error: unknown) => error);
    expect(failure).toMatchObject({
      trace: {
        wallTimeMs: 250,
        reasoning: [{ id: 'thinking', kind: 'thinking', text: 'Public thought' }],
      },
    });
  });
});

it('rejects malformed matching-run events without throwing from the subscription callback', async () => {
  let callbackFailure: unknown;
  native.invoke.mockImplementation((command, args) => {
    if (command === 'cancel_cli') return Promise.resolve();
    try {
      native.receive?.({
        payload: {
          id: args.request!.id,
          kind: 'reasoning',
          elapsedMs: 1,
          reasoning: { id: 'r', kind: 'answer', text: 'Must not be promoted to reasoning' },
        },
      });
    } catch (error) {
      callbackFailure = error;
    }
    return Promise.resolve({
      answer: 'Answer',
      trace: { provider: 'codex', reasoningMs: 1, terminalMs: 0, summary: 'Done' },
    });
  });
  const seen: CliEvent[] = [];
  await expect(
    cliProviders((event) => seen.push(event)).codex.run({
      task: 'Task',
      systemPrompt: 'PERSON',
      effort: 'medium',
    }),
  ).rejects.toThrow();
  expect(callbackFailure).toBeUndefined();
  expect(seen).toEqual([]);
  expect(native.invoke).toHaveBeenCalledWith('cancel_cli', expect.anything());
  expect(native.unlisten).toHaveBeenCalledOnce();
});

it('uses canonical native request keys and decodes the returned trace', async () => {
  native.invoke.mockResolvedValue({
    answer: 'Exact answer  \r\n👩🏽‍🔬',
    trace: {
      provider: 'kimi',
      reasoningMs: 5,
      terminalMs: 0,
      summary: 'Done',
      resolvedModel: null,
    },
  });
  const result = await cliProviders(() => {}).kimi.run({
    task: 'Task',
    systemPrompt: 'PERSON',
    effort: 'medium',
    sourceContext: true,
  });
  expect(native.invoke).toHaveBeenCalledWith('run_cli', {
    request: expect.objectContaining({ systemPrompt: 'PERSON', sourceContext: true }) as unknown,
  });
  expect(native.invoke.mock.calls[0]?.[1].request).not.toHaveProperty('system-prompt');
  expect(result.trace).toMatchObject({ reasoningMs: 5, resolvedModel: null });
  expect(result.answer).toBe('Exact answer  \r\n👩🏽‍🔬');
});

it('uses the host invocation identity to correlate native progress without expanding native request fields', async () => {
  const invocationId = 'c867090a-f7ea-4a1c-ad04-9a4a3920b077';
  const seen: CliEvent[] = [];
  native.invoke.mockImplementation(() => {
    native.receive?.({ payload: { id: invocationId, kind: 'answer', elapsedMs: 3 } });
    return Promise.resolve({
      answer: 'Exact answer',
      trace: { provider: 'codex', reasoningMs: 3, terminalMs: 0, summary: 'Done' },
    });
  });
  const result = await cliProviders((event) => seen.push(event)).codex.run({
    invocationId,
    task: 'Task',
    systemPrompt: 'PERSON',
    effort: 'medium',
  });
  expect(native.invoke).toHaveBeenCalledWith('run_cli', {
    request: expect.objectContaining({ id: invocationId }) as unknown,
  });
  expect(native.invoke.mock.calls[0]?.[1].request).not.toHaveProperty('invocationId');
  expect(result.answer).toBe('Exact answer');
  expect(seen).toEqual([{ id: invocationId, kind: 'answer', elapsedMs: 3 }]);
});

it('reports unconfirmed native cancellation while preserving the original abort cause and public trace', async () => {
  const controller = new AbortController();
  const abortReason = new Error('Caller stopped the run');
  const nativeCancelError = new Error('Native cancellation failed');
  let observedNativeError: unknown;
  const cancellation = Promise.reject(nativeCancelError);
  void cancellation.catch((error: unknown) => {
    observedNativeError = error;
  });
  native.invoke.mockImplementation((command) => {
    if (command === 'cancel_cli') return cancellation;
    controller.abort(abortReason);
    return Promise.reject(abortReason);
  });
  const failure = await cliProviders(() => {})
    .codex.run(
      {
        task: 'Exact task',
        systemPrompt: 'PERSON',
        effort: 'medium',
      },
      controller.signal,
    )
    .catch((error: unknown) => error);
  expect(observedNativeError).toBe(nativeCancelError);
  expect(failure).toMatchObject({
    cause: abortReason,
    message: 'Caller stopped the run Не удалось подтвердить остановку CLI.',
    trace: {
      reasoning: [],
      summary: 'Запрос не завершён: Caller stopped the run Не удалось подтвердить остановку CLI.',
    },
  });
  expect(native.unlisten).toHaveBeenCalledOnce();
});
