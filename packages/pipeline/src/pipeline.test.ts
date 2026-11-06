import { describe, expect, it } from 'vitest';
import { createModelSettings } from '@proteus-int/intelligence';
import type { IntelligenceProvider } from '@proteus-int/intelligence';
import { runExperiment, validateExperiment } from './index';
const input = {
  task: 'Design a public reading room',
  people: [
    { label: 'Ada', context: 'Careful planner', 'public-urls': ['https://example.org/ada'] },
    { label: 'Boris', context: 'Improvises rapidly', 'public-urls': [] },
  ],
} as const;
function providers(failAt = -1) {
  const calls: { task: string; provider: string }[] = [];
  const provider = (id: 'codex' | 'kimi'): IntelligenceProvider => ({
    id,
    async run(request, signal) {
      signal?.throwIfAborted();
      calls.push({ task: request.task, provider: id });
      if (calls.length === failAt)
        return {
          answer: 'not JSON',
          trace: { provider: id, 'reasoning-ms': 1, 'terminal-ms': 0, summary: '' },
        };
      const data = request['system-prompt'].includes('STALKER')
        ? {
            evidence: [
              {
                id: 'e1',
                'source-url': 'https://example.org/ada',
                kind: 'hypothesis',
                summary: 'Prefers structure',
              },
            ],
            'estimated-bsc': 2,
            effort: 'medium',
            character: 'Careful',
            'thinking-style': 'Structured',
            activity: 'Unknown',
            'behavior-summary': 'Plans before acting',
          }
        : request['system-prompt'].includes('PERSON_FABRIC')
          ? { archetype: 'Structured planner', skills: ['Compare options before choosing'] }
          : request['system-prompt'].includes('BRAIN_ANALYZATOR')
            ? {
                summary: 'Different approaches',
                'key-differences': ['Planning depth'],
                people: [
                  { strengths: ['Structure'], weaknesses: ['Speed'], observations: ['Methodical'] },
                  { strengths: ['Speed'], weaknesses: ['Detail'], observations: ['Flexible'] },
                ],
              }
            : { answer: 'A practical reading room plan', summary: 'A plan with three zones' };
      return {
        answer: JSON.stringify(data),
        trace: {
          provider: id,
          'reasoning-ms': 120000,
          'terminal-ms': 600000,
          summary: 'Completed',
        },
      };
    },
  });
  return { calls, adapters: { codex: provider('codex'), kimi: provider('kimi') } };
}
describe('complete two-person experiment', () => {
  it('runs all stages, routes each component, scores reasoning only and emits ordered progress', async () => {
    const { calls, adapters } = providers();
    const stages: string[] = [];
    const settings = {
      ...createModelSettings(),
      components: { ...createModelSettings().components, stalker: 'kimi' as const },
    };
    const result = await runExperiment(input, settings, adapters, {
      onProgress: (event) => stages.push(event.stage),
    });
    expect(result.traces).toHaveLength(7);
    expect(result.traces.map((item) => [item.stage, item.person])).toEqual([
      ['stalker', 0],
      ['person-fabric', 0],
      ['person', 0],
      ['stalker', 1],
      ['person-fabric', 1],
      ['person', 1],
      ['brain-analyzator', null],
    ]);
    expect(calls).toHaveLength(7);
    expect(calls.filter((call) => call.provider === 'kimi')).toHaveLength(2);
    expect(result.people.map((person) => person.score.bsc)).toEqual([1, 1]);
    expect(result.comparison.ratio).toBe(1);
    expect(result.people[0]?.profile.archetype).toBe('Structured planner');
    expect(stages.at(-1)).toBe('complete');
    expect(calls[1]?.task).not.toContain('example.org');
    expect(calls[1]?.task).not.toContain('Ada');
  });
  it('rejects malformed model output without continuing or fabricating results', async () => {
    const { calls, adapters } = providers(2);
    await expect(runExperiment(input, createModelSettings(), adapters)).rejects.toThrow(/JSON/);
    expect(calls).toHaveLength(2);
  });
  it('stops before the next stage when cancelled', async () => {
    const { calls, adapters } = providers();
    const controller = new AbortController();
    await expect(
      runExperiment(input, createModelSettings(), adapters, {
        signal: controller.signal,
        onProgress: (event) => {
          if (event.completed === 1) controller.abort();
        },
      }),
    ).rejects.toThrow();
    expect(calls).toHaveLength(1);
  });
  it.each([
    'http://localhost/private',
    'http://127.0.0.1',
    'https://192.168.1.2',
    'file:///etc/passwd',
    'https://user:secret@example.com',
    'https://[::1]',
  ])('rejects non-public URL %s before contacting a provider', (url) => {
    expect(() =>
      validateExperiment({
        ...input,
        people: [{ ...input.people[0], 'public-urls': [url] }, input.people[1]],
      }),
    ).toThrow();
  });
  it('requires a task and two described people', () => {
    expect(() => validateExperiment({ ...input, task: ' ' })).toThrow();
    expect(() =>
      validateExperiment({
        ...input,
        people: [{ label: 'A', context: '', 'public-urls': [] }, input.people[1]],
      }),
    ).toThrow();
  });
});
