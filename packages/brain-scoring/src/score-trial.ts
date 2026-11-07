import type {
  AnswerEvaluation,
  CriterionScore,
  EvaluationRubric,
  RequiredStatus,
  TrialScore,
} from '@proteus-int/contracts';
import { validateRubric } from './validate-rubric';
import { assessmentInterval } from './score-interval';
export function combineRequired(statuses: readonly RequiredStatus[]): RequiredStatus {
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
