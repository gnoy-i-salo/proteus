import type {
  AnswerEvaluation,
  CriterionAssessment,
  CriterionStatus,
  EvaluationRubric,
} from '@proteus-int/contracts';
import { decodeResponseFields, object, text } from '@proteus-int/intelligence';

export function parseAssessment(
  value: Record<string, unknown>,
  rubric: EvaluationRubric,
  blind: readonly { readonly label: string; readonly answer: string }[],
  answers: readonly { readonly id: string; readonly answer: string }[],
): readonly AnswerEvaluation[] {
  if (!Array.isArray(value.evaluations) || value.evaluations.length !== blind.length)
    throw new Error('Evaluation requires every answer label exactly once');
  const labels = new Set<string>();
  const entries = value.evaluations.map((raw): AnswerEvaluation => {
    const entry = object(raw);
    const label = text(entry.label, 'evaluation.label');
    const index = blind.findIndex((answer) => answer.label === label);
    if (index < 0 || labels.has(label)) throw new Error('Unknown or duplicate evaluation label');
    labels.add(label);
    if (!Array.isArray(entry.criteria) || entry.criteria.length !== rubric.criteria.length)
      throw new Error('Evaluation criteria must match the complete rubric');
    const ids = new Set<string>();
    const criteria = entry.criteria.map((rawCriterion): CriterionAssessment => {
      const criterion = decodeResponseFields(rawCriterion, [['criterion-id', 'criterionId']]);
      const id = text(criterion.criterionId, 'criterionId');
      if (ids.has(id) || !rubric.criteria.some((item) => item.id === id))
        throw new Error('Unknown or duplicate evaluation criteria');
      ids.add(id);
      if (
        typeof criterion.status !== 'string' ||
        !['unmet', 'partial', 'met', 'unknown'].includes(criterion.status)
      )
        throw new Error('Invalid criterion status');
      if (typeof criterion.quote !== 'string' || criterion.quote.length > 24000)
        throw new Error('Evaluation quote must be a string');
      const quote = criterion.quote;
      if (quote && !blind[index]!.answer.includes(quote))
        throw new Error('Evaluation quote must occur literally in its answer');
      if (['met', 'partial'].includes(criterion.status) && !quote.trim())
        throw new Error('Achievement requires a nonempty answer quote');
      return {
        criterionId: id,
        status: criterion.status as CriterionStatus,
        reason: text(criterion.reason, 'assessment.reason'),
        quote,
      };
    });
    return { personId: answers[index]!.id, criteria };
  });
  return answers.map((answer) => entries.find((entry) => entry.personId === answer.id)!);
}
