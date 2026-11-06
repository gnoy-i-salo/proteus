import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { CliEvent } from './cli';
import { cliProviders } from './cli';

const native = vi.hoisted(() => ({
  invoke: vi.fn(),
  listen: vi.fn(),
  unlisten: vi.fn(),
  receive: undefined as ((event: { payload: CliEvent }) => void) | undefined,
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
          'elapsed-ms': 30,
          reasoning: { id: 'r', kind: 'thinking', text: 'я'.repeat(30000) },
        },
      });
      throw new Error('Stopped');
    });
    const failure = await cliProviders(() => {})
      .kimi.run({
        task: 'Task',
        'system-prompt': 'PERSON',
        effort: 'medium',
      })
      .catch((error: unknown) => error);
    expect(failure).toMatchObject({
      trace: {
        'reasoning-truncated': true,
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
            'elapsed-ms': elapsed,
            reasoning: { id: 'r', kind: 'summary', text },
          },
        });
      native.receive?.({
        payload: {
          id: 'unrelated-run',
          kind: 'reasoning',
          'elapsed-ms': 900,
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
          'system-prompt': 'PERSON',
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
        'wall-time-ms': 150,
        'reasoning-ms': 150,
        'active-reasoning-ms': null,
        'requested-model': 'chosen-alias',
        'resolved-model': null,
        'requested-effort': 'high',
        'applied-effort': null,
        'cli-version': null,
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
          'wall-time-ms': 250,
          'reasoning-ms': 250,
          'terminal-ms': 0,
          summary: 'Returned',
          reasoning: [{ id: 'thinking', kind: 'thinking', text: 'Public thought' }],
        },
      };
    });
    const failure = await cliProviders(() => {})
      .kimi.run(
        {
          task: 'Task',
          'system-prompt': 'PERSON',
          effort: 'medium',
        },
        controller.signal,
      )
      .catch((error: unknown) => error);
    expect(failure).toMatchObject({
      trace: {
        'wall-time-ms': 250,
        reasoning: [{ id: 'thinking', kind: 'thinking', text: 'Public thought' }],
      },
    });
  });
});
