import { describe, expect, it } from 'vitest';
import type { IntelligenceRequest, IntelligenceTrace } from '@proteus-int/contracts';
import { createModelSettings, IntelligenceRunError } from '@proteus-int/intelligence';
import type { IntelligenceProvider } from '@proteus-int/intelligence';
import { runExperiment } from '../index';

const input = {
  task: 'Write a short plan',
  people: [
    { label: 'Ada', context: 'Plans', publicUrls: [] },
    { label: 'Boris', context: 'Explores', publicUrls: [] },
  ],
} as const;
const trace = (text: string, duration: number): IntelligenceTrace => ({
  provider: 'codex',
  reasoningMs: duration,
  wallTimeMs: duration,
  terminalMs: 0,
  summary: 'Public trace',
  activeReasoningMs: null,
  reasoning: [{ id: 'r', kind: 'summary', text }],
});
function preparation(request: IntelligenceRequest): unknown {
  const stage = request.systemPrompt.split('\n')[0];
  if (stage === 'BRAIN_ANALYZATOR_RUBRIC')
    return {
      criteria: [
        {
          id: 'coverage',
          label: 'Coverage',
          weight: 1,
          required: false,
          anchors: { unmet: 'No plan', partial: 'Some plan', met: 'Full plan' },
        },
      ],
    };
  if (stage === 'STALKER')
    return {
      evidence: [],
      character: 'unknown',
      thinkingStyle: 'unknown',
      activity: 'unknown',
      behaviorSummary: 'unknown',
    };
  if (stage === 'PERSON_FABRIC') return { rules: [] };
  if (stage === 'PERSON_FABRIC_ADAPT')
    return {
      skills: [
        {
          id: 's1',
          instruction: 'Use a general plan',
          evidenceIds: [],
          status: 'hypothesis',
          limitations: [],
        },
      ],
    };
  throw new Error(`Unexpected stage ${stage}`);
}

describe('partial public traces in experiment results', () => {
  it.each([false, true])(
    'preserves both attempt traces after a format retry; cancellation=%s',
    async (cancelled) => {
      const controller = new AbortController();
      let solves = 0;
      const provider: IntelligenceProvider = {
        id: 'codex',
        run(request) {
          if (request.systemPrompt.startsWith('PERSON\n')) {
            solves++;
            if (solves === 1)
              return Promise.resolve({
                answer: 'invalid JSON',
                trace: trace('First public summary', 10),
              });
            if (cancelled) controller.abort();
            return Promise.reject(
              new IntelligenceRunError('Provider interrupted', {
                ...trace('Partial second public summary', 20),
                reasoningTruncated: true,
              }),
            );
          }
          return Promise.resolve({
            answer: JSON.stringify(preparation(request)),
            trace: trace('Preparation', 5),
          });
        },
      };
      const result = await runExperiment(
        input,
        createModelSettings(),
        {
          codex: provider,
          kimi: provider,
        },
        { signal: controller.signal },
      );
      expect(result.status).toBe(cancelled ? 'cancelled' : 'incomplete');
      const stage = result.traces.find((entry) => entry.id === 'solve-1-a')!;
      expect(stage.trace.wallTimeMs).toBe(30);
      expect(stage.trace.reasoningMs).toBe(30);
      expect(stage.trace.activeReasoningMs).toBeNull();
      expect(stage.trace.reasoningTruncated).toBe(true);
      expect(stage.trace.reasoning).toEqual([
        { id: '1:r', kind: 'summary', text: 'First public summary' },
        { id: '2:r', kind: 'summary', text: 'Partial second public summary' },
      ]);
      expect(stage.attempts.map((attempt) => attempt.status)).toEqual([
        'format-rejected',
        'failed',
      ]);
      expect(stage.attempts.map((attempt) => attempt.wallTimeMs)).toEqual([10, 20]);
      expect(result.trials[0]).toMatchObject({ status: 'failed', attempt: null, score: null });
      expect(result.comparison.comparisonEligible).toBe(false);
      expect(JSON.stringify(result)).toContain('Partial second public summary');
    },
  );
});
