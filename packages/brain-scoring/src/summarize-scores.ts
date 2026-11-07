import type { ScoreSummary, TrialScore } from '@proteus-int/contracts';
import { combineRequired } from './score-trial';
import { median, validateInterval } from './score-interval';
export function summarizeScores(
  personId: string,
  scores: readonly TrialScore[],
  planned: number,
): ScoreSummary {
  if (!personId.trim() || !Number.isInteger(planned) || planned < 1 || scores.length > planned)
    throw new Error('Expected a person, positive planned count and no excess evaluated trials');
  for (const score of scores) {
    if (
      score.version !== 2 ||
      score.personId !== personId ||
      !['passed', 'failed', 'unknown'].includes(score.requiredStatus)
    )
      throw new Error('Invalid version, person or required status in trial score');
    validateInterval(score.interval);
  }
  const complete = scores.length === planned;
  const fullyAssessed = scores.every((score) =>
    score.criteria.every((criterion) => criterion.interval.lower === criterion.interval.upper),
  );
  const interval = scores.length
    ? {
        lower: median(scores.map((score) => score.interval.lower)),
        upper: median(scores.map((score) => score.interval.upper)),
      }
    : null;
  const required = combineRequired([
    ...scores.map((score) => score.requiredStatus),
    ...(!complete ? ['unknown' as const] : []),
  ]);
  return {
    version: 2,
    personId: personId,
    interval,
    point:
      complete && fullyAssessed && interval?.lower === interval?.upper
        ? (interval?.lower ?? null)
        : null,
    planned,
    evaluated: scores.length,
    complete,
    spread: scores.length
      ? {
          lower: Math.min(...scores.map((score) => score.interval.lower)),
          upper: Math.max(...scores.map((score) => score.interval.upper)),
        }
      : null,
    requiredStatus: required,
    judgeDisagreement: scores.some((score) => score.judgeDisagreement),
    comparisonEligible:
      complete &&
      fullyAssessed &&
      required === 'passed' &&
      scores.every(
        (score) => !score.judgeDisagreement && score.interval.lower === score.interval.upper,
      ),
  };
}
