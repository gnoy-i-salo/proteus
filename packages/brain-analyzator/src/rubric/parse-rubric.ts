import type { EvaluationRubric, RubricCriterion } from '@proteus-int/contracts';
import { object, text } from '@proteus-int/intelligence';
export function parseRubric(data: Record<string, unknown>): EvaluationRubric {
  if (!Array.isArray(data.criteria) || data.criteria.length < 1 || data.criteria.length > 12)
    throw new Error('Rubric: expected 1–12 criteria');
  const ids = new Set<string>();
  const criteria = data.criteria.map((value): RubricCriterion => {
    const item = object(value);
    const id = text(item.id, 'criterion.id');
    if (!/^[a-z0-9]+(?:-[a-z0-9]+)*$/u.test(id) || ids.has(id))
      throw new Error('Rubric: unique kebab-case criterion IDs required');
    ids.add(id);
    if (typeof item.weight !== 'number' || !Number.isFinite(item.weight) || item.weight <= 0)
      throw new Error('Rubric: positive finite weight required');
    if (typeof item.required !== 'boolean') throw new Error('Rubric: required must be boolean');
    const anchors = object(item.anchors);
    return Object.freeze({
      id,
      label: text(item.label, 'criterion.label'),
      weight: item.weight,
      required: item.required,
      anchors: Object.freeze({
        unmet: text(anchors.unmet, 'anchors.unmet'),
        partial: text(anchors.partial, 'anchors.partial'),
        met: text(anchors.met, 'anchors.met'),
      }),
    });
  });
  if (!Number.isFinite(criteria.reduce((sum, criterion) => sum + criterion.weight, 0)))
    throw new Error('Rubric: total weight must be finite');
  return Object.freeze({ version: 2, criteria: Object.freeze(criteria) });
}
