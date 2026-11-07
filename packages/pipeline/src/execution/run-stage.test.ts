import { describe, expect, it, vi } from 'vitest';
import { createModelSettings, structured } from '@proteus-int/intelligence';
import type { IntelligenceProvider } from '@proteus-int/intelligence';
import type { IntelligenceRequest } from '@proteus-int/contracts';
import { createStageRunner } from './run-stage';
import type { ProgressEvent, StageTrace } from './types';

const request: IntelligenceRequest = {
  systemPrompt: 'Explicit instructions',
  task: 'Exact answer text',
  effort: 'medium',
};
function provider(answer = '{"value":"ok"}') {
  const run = vi.fn<IntelligenceProvider['run']>().mockResolvedValue({
    answer,
    trace: {
      provider: 'codex',
      reasoningMs: 4,
      wallTimeMs: 4,
      terminalMs: 0,
      summary: 'Observed summary',
      reasoning: [{ id: 'r', kind: 'summary', text: 'Exposed summary' }],
    },
  });
  return { id: 'codex' as const, run };
}

describe('stage execution characterization', () => {
  it('counts a repaired action once while keeping both exposed traces and detached notifications', async () => {
    const adapter = provider();
    const progress: ProgressEvent[] = [];
    const options = {
      onProgress: (event: ProgressEvent) => progress.push(event),
      onTrace: (trace: StageTrace) => {
        Object.assign(trace.trace, { summary: 'Changed observer copy' });
      },
    };
    const runner = createStageRunner(
      {
        settings: createModelSettings(),
        providers: { codex: adapter, kimi: adapter },
        sourceContext: true,
        total: 13,
      },
      options,
    );
    await runner.runStage('source', 'stalker', 0, 'Sources', async (current) => {
      await current.run({ ...request, formatAttempt: 1 });
      return current.run({ ...request, formatAttempt: 2 });
    });
    expect(runner.completed).toBe(1);
    expect(progress.map((event) => event.completed)).toEqual([0, 0, 1]);
    expect(runner.traces[0]?.attempts.map((attempt) => attempt.status)).toEqual([
      'format-rejected',
      'accepted',
    ]);
    expect(runner.traces[0]?.trace).toMatchObject({ wallTimeMs: 8, summary: 'Observed summary' });
    expect(runner.traces[0]?.trace.reasoning).toEqual([
      { id: '1:r', kind: 'summary', text: 'Exposed summary' },
      { id: '2:r', kind: 'summary', text: 'Exposed summary' },
    ]);
    expect(JSON.stringify(runner.traces)).not.toContain('Exact answer text');
    expect(adapter.run.mock.calls.every(([value]) => value.sourceContext === true)).toBe(true);
    const latestObserver = vi.fn();
    options.onTrace = latestObserver;
    await runner.runStage('next', 'person', 0, 'Next', (current) => current.run(request));
    expect(runner.completed).toBe(2);
    expect(latestObserver).toHaveBeenCalled();
  });

  it('retains both rejected format attempts and does not count a failed stage as completed', async () => {
    const adapter = provider('invalid JSON');
    const runner = createStageRunner(
      {
        settings: createModelSettings(),
        providers: { codex: adapter, kimi: adapter },
        sourceContext: false,
        total: 13,
      },
      {},
    );
    await expect(
      runner.runStage('reply', 'person', 0, 'Reply', (current) =>
        structured(current, 'PERSON', '', {}),
      ),
    ).rejects.toThrow(/дважды/);
    expect(runner.completed).toBe(0);
    expect(adapter.run).toHaveBeenCalledTimes(2);
    expect(runner.traces[0]?.attempts.map((attempt) => attempt.status)).toEqual([
      'format-rejected',
      'format-rejected',
    ]);
    expect(runner.traces[0]?.trace.wallTimeMs).toBe(8);
  });

  it('does not start an adapter or emit progress after cancellation', async () => {
    const adapter = provider();
    const controller = new AbortController();
    controller.abort();
    const onProgress = vi.fn();
    const runner = createStageRunner(
      {
        settings: createModelSettings(),
        providers: { codex: adapter, kimi: adapter },
        sourceContext: false,
        total: 13,
      },
      { signal: controller.signal, onProgress },
    );
    await expect(
      runner.runStage('reply', 'person', 0, 'Reply', (current) => current.run(request)),
    ).rejects.toThrow();
    expect(adapter.run).not.toHaveBeenCalled();
    expect(onProgress).not.toHaveBeenCalled();
    expect(runner.traces).toEqual([]);
    expect(runner.completed).toBe(0);
  });
});
