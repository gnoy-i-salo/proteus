import { describe, expect, it, vi } from 'vitest';
import { createModelSettings, structured } from '@proteus-int/intelligence';
import type { IntelligenceProvider } from '@proteus-int/intelligence';
import type { IntelligenceRequest } from '@proteus-int/contracts';
import { createStageRunner } from './run-stage';

const request: IntelligenceRequest = {
  systemPrompt: 'instructions',
  task: 'data',
  effort: 'medium',
};
function setup(answers: string[] = ['{"ok":true}'], options = {}) {
  const run = vi.fn<IntelligenceProvider['run']>().mockImplementation(() =>
    Promise.resolve({
      answer: answers.shift() ?? '{"ok":true}',
      trace: {
        provider: 'codex',
        reasoningMs: 4,
        wallTimeMs: 4,
        terminalMs: 0,
        summary: 'Actual summary',
        reasoning: [],
      },
    }),
  );
  const adapter = { id: 'codex' as const, run };
  const runner = createStageRunner(
    {
      settings: createModelSettings(),
      providers: { codex: adapter, kimi: adapter },
      sourceContext: false,
      total: 13,
    },
    options,
  );
  return { runner, run };
}

describe('logical stages and individual model calls', () => {
  it('records three ordinary STALKER calls without counting a format repair', async () => {
    const { runner, run } = setup();
    await runner.runStage('sources-0', 'stalker', 0, 'Sources', async (provider) => {
      for (const operation of ['source-plan', 'corpus-analysis', 'source-plan'] as const)
        await structured(
          provider,
          'STALKER',
          '',
          {},
          undefined,
          'medium',
          (value) => value,
          operation,
        );
    });
    const execution = runner.snapshot();
    const stage = execution.stages[0]!;
    expect(stage).toMatchObject({
      schemaVersion: 1,
      logicalStageId: 'sources-0',
      status: 'completed',
    });
    expect(stage.calls.map((call) => call.trigger)).toEqual(['initial', 'initial', 'initial']);
    expect(stage.calls.map((call) => call.status)).toEqual(['completed', 'completed', 'completed']);
    expect(new Set(stage.calls.map((call) => call.callId)).size).toBe(3);
    expect(run.mock.calls.map(([value]) => value.invocationId)).toEqual(
      stage.calls.map((call) => call.callId),
    );
    expect(stage.calls.every((call) => call.startedAt !== null && call.finishedAt !== null)).toBe(
      true,
    );
    expect(runner.traces[0]?.lifecycle).toEqual(stage);
    expect(runner.completed).toBe(1);
  });

  it('marks only the second call as format repair and preserves the rejected call', async () => {
    const { runner } = setup(['broken', '{"ok":true}']);
    await runner.runStage('reply', 'person', 0, 'Reply', (p) => structured(p, 'PERSON', '', {}));
    expect(runner.snapshot().stages[0]?.calls).toMatchObject([
      {
        trigger: 'initial',
        formatAttempt: 1,
        status: 'failed',
        validationError: 'Invalid JSON syntax',
      },
      {
        trigger: 'formatRepair',
        formatAttempt: 2,
        status: 'completed',
        validationError: null,
      },
    ]);
    expect(runner.traces[0]?.attempts.map((attempt) => attempt.status)).toEqual([
      'format-rejected',
      'accepted',
    ]);
  });

  it('publishes running and validating before final completion with detached snapshots', async () => {
    const events: { status: string; calls: { status: string }[] }[] = [];
    const { runner } = setup(undefined, {
      onExecution: (event: (typeof events)[number]) => {
        events.push(structuredClone(event));
        Object.assign(event, { status: 'corrupted' });
      },
    });
    await runner.runStage('reply', 'person', 0, 'Reply', (p) => structured(p, 'PERSON', '', {}));
    expect(
      events.some((stage) => stage.status === 'running' && stage.calls[0]?.status === 'running'),
    ).toBe(true);
    expect(
      events.some(
        (stage) => stage.status === 'validating' && stage.calls[0]?.status === 'validating',
      ),
    ).toBe(true);
    expect(events.at(-1)?.status).toBe('completed');
    expect(runner.snapshot().stages[0]?.status).toBe('completed');
  });

  it('keeps a cancelled preparation stage even when no model returned a trace', async () => {
    const controller = new AbortController();
    const { runner } = setup(undefined, { signal: controller.signal });
    await expect(
      runner.runStage('sources-0', 'stalker', 0, 'Sources', () => {
        controller.abort();
        controller.signal.throwIfAborted();

        return Promise.resolve();
      }),
    ).rejects.toThrow();
    expect(runner.snapshot().stages[0]).toMatchObject({
      status: 'cancelled',
      calls: [],
    });
    expect(runner.traces).toEqual([]);
    expect(runner.completed).toBe(0);
  });

  it('distinguishes cancelled and failed calls and explicit skipped stages', async () => {
    const controller = new AbortController();
    const { runner, run } = setup(undefined, { signal: controller.signal });
    run.mockImplementationOnce(() => {
      controller.abort();
      return Promise.reject(new Error('Stopped'));
    });
    await expect(
      runner.runStage('reply', 'person', 0, 'Reply', (p) => p.run(request, controller.signal)),
    ).rejects.toThrow('Stopped');
    runner.skipStage('later', 'person', 1, 'Later', 'Run cancelled');
    expect(runner.snapshot().stages).toMatchObject([
      {
        status: 'cancelled',
        calls: [{ status: 'cancelled', error: 'Stopped' }],
      },
      { status: 'skipped', calls: [], error: 'Run cancelled' },
    ]);
    const failed = setup();
    failed.run.mockRejectedValueOnce(new Error('Provider failed'));
    await expect(
      failed.runner.runStage('reply', 'person', 0, 'Reply', (p) => p.run(request)),
    ).rejects.toThrow('Provider failed');
    expect(failed.runner.snapshot().stages[0]).toMatchObject({
      status: 'failed',
      calls: [{ status: 'failed' }],
    });
  });
});
