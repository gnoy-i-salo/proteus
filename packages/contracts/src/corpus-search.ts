// Shared source-search boundary for the native reference-only FTS index
// (proteus-tip.9). This is bounded metadata search over authorized corpus
// snapshots: it is not MemoryPort, grants no model processing or export rights,
// and carries no original source text. The later host MemoryPort must bind
// persona/grant scope independently at the native host.

// Period bounds are epoch milliseconds; the native side narrows the actual
// persisted grants. Callers cannot supply grantIds, bindingIds, text or chunks.
export interface CorpusSearchPeriod {
  readonly from: number | null;
  readonly to: number | null;
  readonly includeUndated: boolean;
}

export interface CorpusSearchQuery {
  readonly jobId: string;
  readonly snapshotId: string;
  readonly personaId: string;
  // Preserved byte-exact: no trimming and no Unicode normalization anywhere.
  readonly queryText: string;
  readonly period: CorpusSearchPeriod;
  readonly limit: number;
  readonly offset: number;
  // Positive JS-safe index generation pin; required whenever offset > 0.
  readonly expectedIndexGeneration: number | null;
  readonly maxDurationMs: number;
}

// Exact camelCase projection of the native SearchHit DTO. Contains metadata
// and spans only; episode IDs and source text are intentionally absent.
export interface CorpusSearchHit {
  readonly chunkId: string;
  readonly snapshotId: string;
  readonly indexGeneration: number;
  // Finite BM25 rank; negative values are valid (lower is better).
  readonly bm25: number;
  readonly documentId: string;
  readonly revisionId: string;
  readonly spanStart: number;
  readonly spanEnd: number;
  readonly personaId: string;
  readonly grantId: string;
  readonly bindingId: string;
  readonly periodFrom: number | null;
  readonly periodTo: number | null;
  readonly conversationId: string | null;
  readonly threadId: string | null;
}

export interface CorpusSearchResult {
  readonly schemaVersion: 1;
  readonly snapshotId: string;
  readonly snapshotGeneration: number;
  readonly indexGeneration: number;
  readonly hasMore: boolean;
  readonly hits: readonly CorpusSearchHit[];
}

// Exact native SearchIndexReceipt plus the wrapper schemaVersion. `createdAt`
// is the native audit UNIX-seconds string produced by `now_seconds()` at
// publication time — it is not ISO8601 and not an original-source timestamp.
export interface CorpusIndexReceipt {
  readonly schemaVersion: 1;
  readonly snapshotId: string;
  readonly snapshotGeneration: number;
  readonly indexGeneration: number;
  readonly policyFingerprint: string;
  readonly contentSha256: string;
  readonly chunkCount: number;
  readonly createdAt: string;
}

// Implemented by the native runtime. Authorized local metadata search only:
// this port does not grant model processing or export, and the later host
// MemoryPort binds its own persona/grant scope independently.
export interface CorpusSearchPort {
  // Rebuild budget 1..3600000 ms is validated by the native adapter.
  rebuild(
    jobId: string,
    snapshotId: string,
    maxDurationMs: number,
    signal?: AbortSignal,
  ): Promise<CorpusIndexReceipt>;
  search(query: CorpusSearchQuery, signal?: AbortSignal): Promise<CorpusSearchResult>;
}
