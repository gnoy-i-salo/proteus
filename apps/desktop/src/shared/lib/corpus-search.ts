import {
  decodeCorpusIndexReceipt,
  decodeCorpusSearchQuery,
  decodeCorpusSearchResult,
} from '@proteus-int/contracts';
import type { CorpusSearchPort } from '@proteus-int/contracts';
import { requestIngestion } from './ingestion';

function searchId(value: string, maximum: number): string {
  if (!value.trim() || new TextEncoder().encode(value).length > maximum || /\p{Cc}/u.test(value))
    throw new Error('Неверный идентификатор поиска.');
  return value;
}

export const nativeCorpusSearch: CorpusSearchPort = {
  async search(query, signal) {
    const selected = decodeCorpusSearchQuery(query);
    const operationId = crypto.randomUUID();
    return requestIngestion(
      'search_ingestion_corpus',
      { request: { schemaVersion: 1, operationId, ...selected } },
      (value) => {
        const result = decodeCorpusSearchResult(value);
        if (
          result.snapshotId !== selected.snapshotId ||
          (selected.expectedIndexGeneration !== null &&
            result.indexGeneration !== selected.expectedIndexGeneration) ||
          result.hits.length > selected.limit ||
          result.hits.some((hit) => hit.personaId !== selected.personaId)
        )
          throw new Error('Результат поиска не соответствует запросу.');
        return result;
      },
      signal,
      { operationId },
    );
  },
  async rebuild(jobId, snapshotId, maxDurationMs, signal) {
    const selectedJob = searchId(jobId, 128);
    const selectedSnapshot = searchId(snapshotId, 256);
    if (!Number.isSafeInteger(maxDurationMs) || maxDurationMs < 1 || maxDurationMs > 3600000)
      throw new Error('Неверный бюджет индексирования.');
    const operationId = crypto.randomUUID();
    return requestIngestion(
      'rebuild_ingestion_index',
      {
        request: {
          schemaVersion: 1,
          operationId,
          jobId: selectedJob,
          snapshotId: selectedSnapshot,
          maxDurationMs,
        },
      },
      (value) => {
        const result = decodeCorpusIndexReceipt(value);
        if (result.snapshotId !== selectedSnapshot)
          throw new Error('Индекс относится к другому снимку.');
        return result;
      },
      signal,
      { operationId },
    );
  },
};
