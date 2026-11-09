import type { RecallBudget, RecallResult } from './memory';
import type { ResearchSessionDescriptor } from './research-memory';
import { fields, list } from './corpus-wire-validation';
import { decodeRetrievalReceipt } from './memory-retrieval-codec';
import { decodeRecallBudget } from './research-memory-codec';
import { decodeSourceExcerpt } from './source-excerpt-codec';
import { memoryId } from './memory-wire-validation';

export type ResearchRead =
  | { readonly operation: 'recall' }
  | { readonly operation: 'readEpisode'; readonly episodeId: string };
export function decodeResearchRetrieval(
  value: unknown,
  session: Pick<
    ResearchSessionDescriptor,
    'corpusSnapshotId' | 'policyVersion' | 'indexGeneration' | 'mode'
  >,
  budget: RecallBudget,
  read: ResearchRead,
): Pick<RecallResult, 'receipt' | 'sources'> {
  const input = fields(value, ['receipt', 'sources']);
  const raw = input.receipt;
  // Legacy readers are explicit elsewhere; a live native session writes V2.
  if (!raw || typeof raw !== 'object' || !('schemaVersion' in raw) || raw.schemaVersion !== 2)
    throw new Error('Неизвестная версия ответа native-памяти.');
  const receipt = decodeRetrievalReceipt(raw, decodeRecallBudget(budget));
  if (
    receipt.operation !== read.operation ||
    receipt.snapshotId !== session.corpusSnapshotId ||
    receipt.policyVersion !== session.policyVersion ||
    receipt.indexGeneration !== session.indexGeneration ||
    receipt.mode !== session.mode ||
    receipt.retrievalVersion !== 'nativeMemoryRetrieval:1'
  )
    throw new Error('Ответ памяти не соответствует открытой сессии.');
  const values = list(input.sources);
  if (values.length !== receipt.hits.length)
    throw new Error('Не все выдержки соответствуют квитанции.');
  const requestedEpisode = read.operation === 'readEpisode' ? memoryId(read.episodeId) : null;
  const sources = values.map(decodeSourceExcerpt);
  let sourceBytes = 0;
  sources.forEach((source, index) => {
    const hit = receipt.hits[index];
    const span = hit?.spans[0];
    if (
      !hit ||
      !span ||
      hit.spans.length !== 1 ||
      (requestedEpisode !== null && hit.episodeId !== requestedEpisode) ||
      span.documentId !== source.span.documentId ||
      span.revisionId !== source.span.revisionId ||
      span.unit !== source.span.unit ||
      span.start !== source.span.start ||
      span.end !== source.span.end
    )
      throw new Error('Выдержка не соответствует запрошенному эпизоду или квитанции.');
    sourceBytes += source.span.end - source.span.start;
  });
  if (sourceBytes !== receipt.sourceBytes)
    throw new Error('Размер выдержек не соответствует квитанции.');
  return { receipt, sources };
}
