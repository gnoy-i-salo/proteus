import type { CriterionAssessment, ScoreInterval } from '@proteus-int/contracts';
export function assessmentInterval(assessment: CriterionAssessment): ScoreInterval {
  if (!assessment.reason?.trim() || typeof assessment.quote !== 'string')
    throw new Error('Criterion assessment requires a reason and quote string');
  if (assessment.status === 'unknown') return { lower: 0, upper: 1 };
  if (!['unmet', 'partial', 'met'].includes(assessment.status))
    throw new Error('Invalid criterion status');
  if (assessment.status !== 'unmet' && !assessment.quote.trim())
    throw new Error('Assessed achievement requires an answer quote');
  const value = assessment.status === 'met' ? 1 : assessment.status === 'partial' ? 0.5 : 0;
  return { lower: value, upper: value };
}
export function validateInterval(interval: ScoreInterval): void {
  if (
    ![interval.lower, interval.upper].every(Number.isFinite) ||
    interval.lower < 0 ||
    interval.upper > 100 ||
    interval.lower > interval.upper
  )
    throw new Error('Expected a finite ordered BSC interval within 0–100');
}
export function median(values: readonly number[]): number {
  const sorted = [...values].sort((a, b) => a - b);
  const middle = Math.floor(sorted.length / 2);
  return sorted.length % 2 ? sorted[middle]! : (sorted[middle - 1]! + sorted[middle]!) / 2;
}
export function difference(a: ScoreInterval, b: ScoreInterval): ScoreInterval {
  validateInterval(a);
  validateInterval(b);
  return { lower: a.lower - b.upper, upper: a.upper - b.lower };
}
