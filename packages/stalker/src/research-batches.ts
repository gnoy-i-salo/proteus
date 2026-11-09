import type { MemoryItemDraft, RecallResult } from '@proteus-int/contracts';
import { parseMemoryAnalysis } from './parse-memory-analysis';
import type { MemoryAnalysisScope } from './parse-memory-analysis';

export type ResearchRecord = Pick<RecallResult, 'receipt' | 'sources'>;
export interface ResearchBatch {
  readonly receiptIds: string[];
  readonly scope: MemoryAnalysisScope;
}
// Leave room for host instructions and persona context within native's 200k task cap.
// Native still measures exact framed input and owns the cumulative model budget.
const maxContextBytes = 128000;
export function researchBatches(
  records: readonly ResearchRecord[],
  maxItems: number,
): { batches: ResearchBatch[]; skipped: number } {
  const batches: ResearchBatch[] = [];
  let skipped = 0;
  let bytes = 0;
  let current: ResearchBatch | undefined;
  for (const record of records) {
    if (record.sources.length !== record.receipt.hits.length)
      throw new Error('Квитанция исследования не соответствует источникам.');
    if (!record.sources.length) continue;
    const size = new TextEncoder().encode(JSON.stringify(record)).length;
    if (
      size > maxContextBytes ||
      record.sources.length > 100 ||
      record.sources.some((source) => source.span.end - source.span.start > 65536)
    ) {
      skipped++;
      continue;
    }
    const sources = record.sources.map((excerpt, index) => ({
      excerpt,
      episodeId: record.receipt.hits[index]!.episodeId,
    }));
    if (
      !current ||
      bytes + size > maxContextBytes ||
      current.scope.sources.length + sources.length > 100
    ) {
      current = { receiptIds: [], scope: { sources: [], existingItemIds: [], maxItems } };
      batches.push(current);
      bytes = 0;
    }
    current.receiptIds.push(record.receipt.id);
    const scope = { ...current.scope, sources: [...current.scope.sources, ...sources] };
    // Reuse the parser's source-boundary validation before any model call.
    parseMemoryAnalysis({ items: [] }, scope);
    const updated = { receiptIds: current.receiptIds, scope };
    batches[batches.length - 1] = updated;
    current = updated;
    bytes += size;
  }
  return { batches, skipped };
}
export function assignDraftIds(
  items: readonly MemoryItemDraft[],
  batch: number,
): readonly MemoryItemDraft[] {
  const ids = new Map(
    items.map((item, index) => [item.draftId, `researchDraft:${batch}:${index}`]),
  );
  return items.map((item) => ({
    ...item,
    draftId: ids.get(item.draftId)!,
    contradictions: item.contradictions.map((ref) => {
      if (ref.kind === 'published') return ref;
      const draftId = ids.get(ref.draftId);
      if (!draftId) throw new Error('Противоречие ссылается на отсутствующий элемент.');
      return { kind: 'draft' as const, draftId };
    }),
  }));
}
