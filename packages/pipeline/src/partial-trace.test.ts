import { describe, expect, it } from 'vitest';
import type { IntelligenceRequest, IntelligenceTrace } from '@proteus-int/contracts';
import { createModelSettings, IntelligenceRunError } from '@proteus-int/intelligence';
import type { IntelligenceProvider } from '@proteus-int/intelligence';
import { runExperiment } from './index';

const input = {
  task: 'Write a short plan',
  people: [
    { label: 'Ada', context: 'Plans', 'public-urls': [] },
    { label: 'Boris', context: 'Explores', 'public-urls': [] },
  ],
} as const;
const trace = (text: string, duration: number): IntelligenceTrace => ({
  provider: 'codex',
  'reasoning-ms': duration,
  'wall-time-ms': duration,
  'terminal-ms': 0,
  summary: 'Public trace',
  'active-reasoning-ms': null,
  reasoning: [{ id: 'r', kind: 'summary', text }],
});
function preparation(request: IntelligenceRequest): unknown {
  const stage = request['system-prompt'].split('\n')[0];
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
      'thinking-style': 'unknown',
      activity: 'unknown',
      'behavior-summary': 'unknown',
    };
  if (stage === 'PERSON_FABRIC') return { rules: [] };
  if (stage === 'PERSON_FABRIC_ADAPT')
    return {
      skills: [
        {
          id: 's1',
          instruction: 'Use a general plan',
          'evidence-ids': [],
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
        async run(request) {
          if (request['system-prompt'].startsWith('PERSON\n')) {
            solves++;
            if (solves === 1)
              return { answer: 'invalid JSON', trace: trace('First public summary', 10) };
            if (cancelled) controller.abort();
            throw new IntelligenceRunError('Provider interrupted', {
              ...trace('Partial second public summary', 20),
              'reasoning-truncated': true,
            });
          }
          return { answer: JSON.stringify(preparation(request)), trace: trace('Preparation', 5) };
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
      expect(stage.trace['wall-time-ms']).toBe(30);
      expect(stage.trace['reasoning-ms']).toBe(30);
      expect(stage.trace['active-reasoning-ms']).toBeNull();
      expect(stage.trace['reasoning-truncated']).toBe(true);
      expect(stage.trace.reasoning).toEqual([
        { id: '1:r', kind: 'summary', text: 'First public summary' },
        { id: '2:r', kind: 'summary', text: 'Partial second public summary' },
      ]);
      expect(stage.attempts.map((attempt) => attempt.status)).toEqual([
        'format-rejected',
        'failed',
      ]);
      expect(stage.attempts.map((attempt) => attempt['wall-time-ms'])).toEqual([10, 20]);
      expect(result.trials[0]).toMatchObject({ status: 'failed', attempt: null, score: null });
      expect(result.comparison['comparison-eligible']).toBe(false);
      expect(JSON.stringify(result)).toContain('Partial second public summary');
    },
  );
});
