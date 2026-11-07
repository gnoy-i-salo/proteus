import { describe, expect, it } from 'vitest';
import type { AnswerEvaluation, EvaluationRubric } from '@proteus-int/contracts';
import { difference, scoreTrial, summarizeScores } from './index';

const rubric: EvaluationRubric = {
  version: 2,
  criteria: [50, 30, 20].map((weight, index) => ({
    id: `c-${index}`,
    label: `Criterion ${index}`,
    weight,
    required: index === 0,
    anchors: { unmet: 'Missing', partial: 'Partly present', met: 'Fully present' },
  })),
};
function pass(statuses: ('unmet' | 'partial' | 'met' | 'unknown')[]): AnswerEvaluation {
  return {
    personId: 'a',
    criteria: statuses.map((status, index) => ({
      criterionId: `c-${index}`,
      status,
      reason: 'Observable evidence',
      quote: status === 'met' || status === 'partial' ? 'An answer passage' : '',
    })),
  };
}
const exact = (statuses: ('unmet' | 'partial' | 'met' | 'unknown')[]) => {
  const evaluation = pass(statuses);
  return scoreTrial('a', rubric, [evaluation, evaluation]);
};
describe('BSC v2 criterion attainment', () => {
  it('normalizes tiny positive weights before applying partial credit', () => {
    const tiny: EvaluationRubric = {
      version: 2,
      criteria: [{ ...rubric.criteria[0]!, weight: Number.MIN_VALUE }],
    };
    const partial = pass(['partial']);
    expect(scoreTrial('a', tiny, [partial, partial]).interval).toEqual({ lower: 50, upper: 50 });
  });
  it('preserves an unknown criterion flag even below aggregate floating-point precision', () => {
    const uneven: EvaluationRubric = {
      version: 2,
      criteria: rubric.criteria.map((criterion, index) => ({
        ...criterion,
        weight: index === 1 ? Number.MIN_VALUE : 1,
      })),
    };
    const evaluation = pass(['met', 'unknown', 'met']);
    const score = scoreTrial('a', uneven, [evaluation, evaluation]);
    expect(summarizeScores('a', [score], 1)).toMatchObject({
      point: null,
      comparisonEligible: false,
    });
  });
  it('retains unknown weight in a 65–85 interval instead of renormalizing', () => {
    const score = exact(['met', 'partial', 'unknown']);
    expect(score.interval).toEqual({ lower: 65, upper: 85 });
    expect(score.requiredStatus).toBe('passed');
  });
  it('keeps judge disagreement and required eligibility separate from arithmetic', () => {
    const score = scoreTrial('a', rubric, [
      pass(['met', 'met', 'met']),
      pass(['partial', 'met', 'met']),
    ]);
    expect(score.interval).toEqual({ lower: 75, upper: 100 });
    expect(score.judgeDisagreement).toBe(true);
    expect(score.requiredStatus).toBe('unknown');
    expect(exact(['unmet', 'met', 'met']).requiredStatus).toBe('failed');
  });
  it('validates IDs, positive finite weights, statuses and both complete passes', () => {
    const valid = pass(['met', 'met', 'met']);
    for (const weight of [0, -1, Infinity, NaN]) {
      expect(() =>
        scoreTrial(
          'a',
          {
            ...rubric,
            criteria: rubric.criteria.map((item, index) =>
              index === 0 ? { ...item, weight } : item,
            ),
          },
          [valid, valid],
        ),
      ).toThrow(/weight/i);
    }
    expect(() =>
      scoreTrial('a', { ...rubric, criteria: [rubric.criteria[0]!, rubric.criteria[0]!] }, [
        valid,
        valid,
      ]),
    ).toThrow(/id/i);
    expect(() => scoreTrial('a', rubric, [valid])).toThrow(/two/i);
    expect(() => scoreTrial('a', rubric, [valid, { ...valid, personId: 'b' }])).toThrow(/person/i);
    expect(() =>
      scoreTrial('a', rubric, [valid, { ...valid, criteria: valid.criteria.slice(1) }]),
    ).toThrow(/criteria/i);
    expect(() =>
      scoreTrial('a', rubric, [
        valid,
        { ...valid, criteria: [valid.criteria[0]!, valid.criteria[0]!, valid.criteria[2]!] },
      ]),
    ).toThrow(/criteria/i);
    expect(() =>
      scoreTrial('a', rubric, [
        valid,
        {
          ...valid,
          criteria: valid.criteria.map((item) => ({ ...item, status: 'excellent' as 'met' })),
        },
      ]),
    ).toThrow(/status/i);
  });
  it('aggregates medians with completeness, spread and required gates', () => {
    const scores = [
      exact(['met', 'unmet', 'unmet']),
      exact(['met', 'met', 'met']),
      exact(['met', 'partial', 'unknown']),
    ];
    expect(summarizeScores('a', scores, 3)).toMatchObject({
      interval: { lower: 65, upper: 85 },
      point: null,
      evaluated: 3,
      planned: 3,
      complete: true,
      spread: { lower: 50, upper: 100 },
      comparisonEligible: false,
    });
    expect(summarizeScores('a', scores, 4)).toMatchObject({
      complete: false,
      evaluated: 3,
      comparisonEligible: false,
    });
    expect(summarizeScores('a', [], 3)).toMatchObject({
      interval: null,
      point: null,
      evaluated: 0,
      complete: false,
      spread: null,
      requiredStatus: 'unknown',
    });
    expect(summarizeScores('a', [exact(['unmet', 'met', 'met'])], 1).comparisonEligible).toBe(
      false,
    );
    expect(() => summarizeScores('a', scores, 2)).toThrow();
  });
  it('computes a baseline difference in percentage points and validates intervals', () => {
    expect(difference({ lower: 65, upper: 85 }, { lower: 60, upper: 70 })).toEqual({
      lower: -5,
      upper: 25,
    });
    expect(() => difference({ lower: 90, upper: 80 }, { lower: 0, upper: 1 })).toThrow();
  });
  it('does not allow comparison when a collapsed median hides unknown or disputed trials', () => {
    const certain = exact(['met', 'met', 'met']);
    const unknown = exact(['met', 'met', 'unknown']);
    const disputed = scoreTrial('a', rubric, [
      pass(['met', 'met', 'met']),
      pass(['met', 'met', 'partial']),
    ]);
    expect(summarizeScores('a', [certain, certain, unknown], 3)).toMatchObject({
      interval: { lower: 100, upper: 100 },
      comparisonEligible: false,
    });
    expect(summarizeScores('a', [certain, certain, disputed], 3).comparisonEligible).toBe(false);
    expect(summarizeScores('a', [certain, certain, certain], 3).comparisonEligible).toBe(true);
  });
});
