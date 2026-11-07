import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { CliEvent } from './cli';
import { cliProviders } from './cli';

const native = vi.hoisted(() => ({
  invoke: vi.fn(),
  listen: vi.fn(),
  unlisten: vi.fn(),
  receive: undefined as ((event: { payload: unknown }) => void) | undefined,
}));
vi.mock('@tauri-apps/api/core', () => ({ invoke: native.invoke }));
vi.mock('@tauri-apps/api/event', () => ({ listen: native.listen }));
vi.mock('./window-controls', () => ({ isDesktop: () => true }));

beforeEach(() => {
  native.receive = undefined;
  native.listen.mockImplementation(async (_name, receive) => {
    native.receive = receive;
    return native.unlisten;
  });
});

describe('failed native calls retain public traces', () => {
  it('bounds partial UTF-8 text without splitting a character', async () => {
    native.invoke.mockImplementation(async (_command, args) => {
      native.receive?.({
        payload: {
          id: args.request.id,
          kind: 'reasoning',
          elapsedMs: 30,
          reasoning: { id: 'r', kind: 'thinking', text: 'я'.repeat(30000) },
        },
      });
      throw new Error('Stopped');
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
    native.invoke.mockImplementation(async (command, args) => {
      if (command === 'cancel_cli') return;
      const id = args.request.id as string;
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
      throw new Error('CLI cancelled');
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
    native.invoke.mockImplementation(async (command) => {
      if (command === 'cancel_cli') return;
      controller.abort();
      return {
        answer: 'Not accepted after cancellation',
        trace: {
          provider: 'kimi',
          wallTimeMs: 250,
          reasoningMs: 250,
          terminalMs: 0,
          summary: 'Returned',
          reasoning: [{ id: 'thinking', kind: 'thinking', text: 'Public thought' }],
        },
      };
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
  native.invoke.mockImplementation(async (command, args) => {
    if (command === 'cancel_cli') return;
    try {
      native.receive?.({
        payload: {
          id: args.request.id,
          kind: 'reasoning',
          elapsedMs: 1,
          reasoning: { id: 'r', kind: 'answer', text: 'Must not be promoted to reasoning' },
        },
      });
    } catch (error) {
      callbackFailure = error;
    }
    return {
      answer: 'Answer',
      trace: { provider: 'codex', reasoningMs: 1, terminalMs: 0, summary: 'Done' },
    };
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
    request: expect.objectContaining({ systemPrompt: 'PERSON', sourceContext: true }),
  });
  expect(native.invoke.mock.calls[0]?.[1].request).not.toHaveProperty('system-prompt');
  expect(result.trace).toMatchObject({ reasoningMs: 5, resolvedModel: null });
  expect(result.answer).toBe('Exact answer  \r\n👩🏽‍🔬');
});
