import { describe, expect, it, vi } from 'vitest';
import type { EvaluationRubric } from '@proteus-int/contracts';
import type { IntelligenceProvider } from '@proteus-int/intelligence';
import { createRubric, evaluateAnswers } from './index';

const rubric: EvaluationRubric = {
  version: 2,
  criteria: [
    {
      id: 'coverage',
      label: 'Task coverage',
      weight: 1,
      required: true,
      anchors: { unmet: 'No solution', partial: 'Incomplete solution', met: 'Complete solution' },
    },
  ],
};
function provider(data: unknown) {
  const run = vi.fn<IntelligenceProvider['run']>(async () => ({
    answer: JSON.stringify(data),
    trace: { provider: 'codex' as const, reasoningMs: 50, terminalMs: 0, summary: 'Done' },
  }));
  return { id: 'codex' as const, run } satisfies IntelligenceProvider;
}
const assessment = {
  criterionId: 'coverage',
  status: 'met',
  reason: 'The answer supplies a plan',
  quote: 'Plan',
};
describe('blind criterion evaluation', () => {
  it('creates a frozen task-only rubric with anchored criteria', async () => {
    const adapter = provider({ criteria: rubric.criteria });
    const result = await createRubric('Make a plan', adapter);
    expect(result.rubric).toEqual(rubric);
    expect(Object.isFrozen(result.rubric.criteria[0]?.anchors)).toBe(true);
    expect(JSON.parse(adapter.run.mock.calls[0]![0]!.task)).toEqual({ task: 'Make a plan' });
  });
  it('rejects duplicated rubric IDs and nonpositive weights', async () => {
    await expect(
      createRubric('Task', provider({ criteria: [rubric.criteria[0], rubric.criteria[0]] })),
    ).rejects.toThrow(/unique/i);
    await expect(
      createRubric('Task', provider({ criteria: [{ ...rubric.criteria[0], weight: 0 }] })),
    ).rejects.toThrow(/weight/i);
  });
  it('strips identities and metadata, mapping positional labels back after a reversed pass', async () => {
    const adapter = provider({
      evaluations: [
        { label: 'answer-2', criteria: [{ ...assessment, status: 'unmet', quote: '' }] },
        { label: 'answer-1', criteria: [assessment] },
      ],
    });
    const answers = [
      { id: 'private-boris', answer: 'Plan B', provider: 'kimi', score: 99 },
      { id: 'private-ada', answer: 'No useful output', provider: 'codex', score: 1 },
    ];
    const result = await evaluateAnswers('Task', rubric, answers, adapter);
    expect(result.evaluations.map((item) => item.personId)).toEqual([
      'private-boris',
      'private-ada',
    ]);
    const payload = JSON.parse(adapter.run.mock.calls[0]![0]!.task);
    expect(payload.answers).toEqual([
      { label: 'answer-1', answer: 'Plan B' },
      { label: 'answer-2', answer: 'No useful output' },
    ]);
    expect(JSON.stringify(payload)).not.toContain('private-');
    expect(JSON.stringify(payload)).not.toContain('provider');
  });
  it('requires actual answer quotations and complete criterion/participant coverage', async () => {
    const answers = [{ id: 'a', answer: 'Plan' }];
    await expect(
      evaluateAnswers(
        'Task',
        rubric,
        answers,
        provider({
          evaluations: [
            { label: 'answer-1', criteria: [{ ...assessment, quote: 'fabricated quote' }] },
          ],
        }),
      ),
    ).rejects.toThrow(/quote/i);
    await expect(
      evaluateAnswers(
        'Task',
        rubric,
        answers,
        provider({ evaluations: [{ label: 'answer-1', criteria: [] }] }),
      ),
    ).rejects.toThrow(/criteria/i);
    await expect(
      evaluateAnswers(
        'Task',
        rubric,
        answers,
        provider({ evaluations: [{ label: 'wrong', criteria: [assessment] }] }),
      ),
    ).rejects.toThrow(/label/i);
    const result = await evaluateAnswers(
      'Task',
      rubric,
      answers,
      provider({
        evaluations: [
          { label: 'answer-1', criteria: [{ ...assessment, status: 'unknown', quote: '' }] },
        ],
      }),
    );
    expect(result.evaluations[0]?.criteria[0]?.status).toBe('unknown');
  });
  it('rejects a status array instead of coercing it into a valid status', async () => {
    await expect(
      evaluateAnswers(
        'Task',
        rubric,
        [{ id: 'a', answer: 'Plan' }],
        provider({
          evaluations: [{ label: 'answer-1', criteria: [{ ...assessment, status: ['met'] }] }],
        }),
      ),
    ).rejects.toThrow(/status/i);
  });
});

describe('assessment response boundary', () => {
  it.each([{ 'criterion-id': 'coverage' }, { criterionId: 'coverage' }])(
    'accepts an explicit legacy or canonical criterion schema',
    async (field) => {
      const result = await evaluateAnswers(
        'Task',
        rubric,
        [{ id: 'a', answer: 'Plan' }],
        provider({
          evaluations: [
            {
              label: 'answer-1',
              criteria: [{ ...field, status: 'met', reason: 'Literal plan', quote: 'Plan' }],
            },
          ],
        }),
      );
      expect(result.evaluations[0]?.criteria[0]?.criterionId).toBe('coverage');
      expect(result.evaluations[0]?.criteria[0]).not.toHaveProperty('criterion-id');
    },
  );
  it('rejects mixed schemas even when aliases have equal values', async () => {
    await expect(
      evaluateAnswers(
        'Task',
        rubric,
        [{ id: 'a', answer: 'Plan' }],
        provider({
          evaluations: [
            { label: 'answer-1', criteria: [{ ...assessment, 'criterion-id': 'coverage' }] },
          ],
        }),
      ),
    ).rejects.toThrow(/Mixed/);
  });
  it('preserves exact quote bytes including CRLF and edge whitespace', async () => {
    const quote = '  План 👩🏽‍💻\r\n';
    const result = await evaluateAnswers(
      'Task',
      rubric,
      [{ id: 'a', answer: quote }],
      provider({
        evaluations: [{ label: 'answer-1', criteria: [{ ...assessment, quote }] }],
      }),
    );
    expect(result.evaluations[0]?.criteria[0]?.quote).toBe(quote);
  });
});
