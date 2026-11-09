import { decodeCorpusEpisodeQuery, decodeCorpusEpisodeResult } from '@proteus-int/contracts';
import type { CorpusInspectionPort } from '@proteus-int/contracts';
import { requestIngestion } from './ingestion';

export const nativeCorpusInspection: CorpusInspectionPort = {
  async inspectEpisode(query, signal) {
    const selected = decodeCorpusEpisodeQuery(query);
    const operationId = crypto.randomUUID();
    return requestIngestion(
      'inspect_ingestion_episode',
      { request: { schemaVersion: 1, operationId, ...selected } },
      (value) => {
        const result = decodeCorpusEpisodeResult(value);
        if (
          result.snapshotId !== selected.snapshotId ||
          result.personaId !== selected.personaId ||
          result.episodeId !== selected.episodeId ||
          result.indexGeneration !== selected.expectedIndexGeneration
        )
          throw new Error('Источник не соответствует закреплённой области просмотра.');
        return result;
      },
      signal,
      { operationId },
    );
  },
};
