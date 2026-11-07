import type { EvaluationRubric } from '@proteus-int/contracts';
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
