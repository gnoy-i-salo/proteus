import type { SourceExcerpt } from './corpus';
import type { CorpusSearchPeriod } from './corpus-search';

/** Local reference-only inspection; grants no model-processing or export permission. */
export interface CorpusEpisodeQuery {
  readonly jobId: string;
  readonly snapshotId: string;
  readonly personaId: string;
  readonly episodeId: string;
  readonly period: CorpusSearchPeriod;
  readonly expectedIndexGeneration: number;
  readonly maxDurationMs: number;
}
export interface CorpusEpisodeResult {
  readonly schemaVersion: 1;
  readonly snapshotId: string;
  readonly indexGeneration: number;
  readonly personaId: string;
  readonly episodeId: string;
  readonly sourceBytes: number;
  readonly contextTokens: number;
  readonly chunks: readonly {
    readonly chunkId: string;
    readonly excerpt: SourceExcerpt;
  }[];
}
export interface CorpusInspectionPort {
  inspectEpisode(query: CorpusEpisodeQuery, signal?: AbortSignal): Promise<CorpusEpisodeResult>;
}
