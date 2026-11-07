import type { SourceDocument } from '@proteus-int/contracts';

export class CollectionStop extends Error {}
export function isSourceBudgetFailure(error: unknown): boolean {
  return (error instanceof Error ? error.message : error) === 'source-budget-exceeded';
}

export class AnalysisLimit extends Error {}

export function collectionStepBudget(maxSteps: number) {
  const analysisSteps = Math.min(maxSteps - 1, Math.max(2, Math.min(4, Math.floor(maxSteps / 3))));
  const collectionSteps = maxSteps - analysisSteps;
  return { analysisSteps, collectionSteps };
}

export function sourceDocumentBatches(documents: readonly SourceDocument[]): SourceDocument[][] {
  const result: SourceDocument[][] = [];
  let current: SourceDocument[] = [];
  let size = 0;
  for (const doc of documents) {
    if (current.length && (size + doc.text.length > 24000 || current.length >= 20)) {
      result.push(current);
      current = [];
      size = 0;
    }
    current.push(doc);
    size += doc.text.length;
  }
  if (current.length) result.push(current);
  return result;
}
