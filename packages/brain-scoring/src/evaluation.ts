import type {
  AnswerEvaluation,
  CriterionAssessment,
  CriterionScore,
  EvaluationRubric,
  RequiredStatus,
  ScoreInterval,
  ScoreSummary,
  TrialScore,
} from '@proteus-int/contracts';

export function validateRubric(rubric: EvaluationRubric): void {
  if (
    rubric.version !== 2 ||
    !Array.isArray(rubric.criteria) ||
    !rubric.criteria.length ||
    rubric.criteria.length > 12
  )
    throw new Error('BSC v2 requires 1–12 rubric criteria');
  const ids = new Set<string>();
  for (const criterion of rubric.criteria) {
    if (!/^[a-z0-9]+(?:-[a-z0-9]+)*$/u.test(criterion.id) || ids.has(criterion.id))
      throw new Error('Rubric criterion IDs must be unique kebab-case IDs');
    ids.add(criterion.id);
    if (!Number.isFinite(criterion.weight) || criterion.weight <= 0)
      throw new Error('Rubric weight must be positive and finite');
    if (
      typeof criterion.required !== 'boolean' ||
      !criterion.label?.trim() ||
      !criterion.anchors?.unmet?.trim() ||
      !criterion.anchors?.partial?.trim() ||
      !criterion.anchors?.met?.trim()
    )
      throw new Error('Rubric requires labels, anchors and required flags');
  }
  if (!Number.isFinite(rubric.criteria.reduce((total, criterion) => total + criterion.weight, 0)))
    throw new Error('Rubric total weight must be finite');
}
function assessmentInterval(assessment: CriterionAssessment): ScoreInterval {
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
function combineRequired(statuses: readonly RequiredStatus[]): RequiredStatus {
  return statuses.includes('failed')
    ? 'failed'
    : statuses.includes('unknown')
      ? 'unknown'
      : 'passed';
}
export function scoreTrial(
  personId: string,
  rubric: EvaluationRubric,
  passes: readonly AnswerEvaluation[],
): TrialScore {
  validateRubric(rubric);
  if (!personId.trim()) throw new Error('Person ID is required');
  if (passes.length !== 2) throw new Error('BSC v2 requires two evaluation passes');
  for (const pass of passes) {
    if (pass.personId !== personId) throw new Error('Evaluation person ID mismatch');
    const ids = new Set(pass.criteria.map((item) => item.criterionId));
    if (
      pass.criteria.length !== rubric.criteria.length ||
      ids.size !== rubric.criteria.length ||
      rubric.criteria.some((criterion) => !ids.has(criterion.id))
    )
      throw new Error('Evaluation criteria must match every rubric criterion exactly once');
  }
  const criteria: CriterionScore[] = rubric.criteria.map((criterion) => {
    const assessments = passes.map((pass) =>
      pass.criteria.find((item) => item.criterionId === criterion.id)!,
    );
    const intervals = assessments.map(assessmentInterval);
    return {
      criterionId: criterion.id,
      assessments,
      disputed: new Set(assessments.map((assessment) => assessment.status)).size > 1,
      interval: {
        lower: Math.min(...intervals.map((value) => value.lower)),
        upper: Math.max(...intervals.map((value) => value.upper)),
      },
    };
  });
  const total = rubric.criteria.reduce((sum, criterion) => sum + criterion.weight, 0);
  const weighted = (endpoint: 'lower' | 'upper') =>
    criteria.reduce(
      (sum, score, index) =>
        sum + (rubric.criteria[index]!.weight / total) * (score.interval[endpoint] * 100),
      0,
    );
  const required = rubric.criteria.flatMap((criterion, index): RequiredStatus[] => {
    if (!criterion.required) return [];
    const interval = criteria[index]!.interval;
    return [interval.lower === 1 ? 'passed' : interval.upper < 1 ? 'failed' : 'unknown'];
  });
  return {
    version: 2,
    personId: personId,
    interval: { lower: Math.min(100, weighted('lower')), upper: Math.min(100, weighted('upper')) },
    criteria,
    requiredStatus: combineRequired(required),
    judgeDisagreement: criteria.some((criterion) => criterion.disputed),
  };
}
function validateInterval(interval: ScoreInterval): void {
  if (
    ![interval.lower, interval.upper].every(Number.isFinite) ||
    interval.lower < 0 ||
    interval.upper > 100 ||
    interval.lower > interval.upper
  )
    throw new Error('Expected a finite ordered BSC interval within 0–100');
}
function median(values: readonly number[]): number {
  const sorted = [...values].sort((a, b) => a - b);
  const middle = Math.floor(sorted.length / 2);
  return sorted.length % 2 ? sorted[middle]! : (sorted[middle - 1]! + sorted[middle]!) / 2;
}
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
export function difference(a: ScoreInterval, b: ScoreInterval): ScoreInterval {
  validateInterval(a);
  validateInterval(b);
  return { lower: a.lower - b.upper, upper: a.upper - b.lower };
}
