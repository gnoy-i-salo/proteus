import type { RevisionReference, SourceSpan } from './corpus';

export interface EmbeddingFingerprint {
  readonly modelId: string;
  readonly modelRevision: string;
  readonly modelSha256: string;
  readonly tokenizerSha256: string;
  readonly dimensions: number;
  readonly pooling: 'mean' | 'cls';
  readonly normalization: 'l2' | 'none';
  readonly queryTemplate: string;
  readonly passageTemplate: string;
  readonly chunkingVersion: string;
}

export interface EmbeddingRecord {
  readonly schemaVersion: 1;
  readonly chunkId: string;
  readonly generation: number;
  readonly fingerprint: EmbeddingFingerprint;
  readonly vector: readonly number[];
}

export interface EmbeddingBudget {
  readonly maxBatchTokens: number;
  readonly maxBatchItems: number;
  readonly maxResidentBytes: number;
  readonly maxDurationMs: number;
}

// Implemented by the native runtime. Missing local weights never trigger a cloud fallback.
export interface EmbeddingPort {
  status(signal?: AbortSignal): Promise<
    | {
        readonly status: 'unavailable';
        readonly reason: 'missingModel' | 'invalidChecksum' | 'unsupportedRuntime';
      }
    | { readonly status: 'ready'; readonly fingerprint: EmbeddingFingerprint }
  >;
  embed(
    texts: readonly string[],
    kind: 'query' | 'passage',
    fingerprint: EmbeddingFingerprint,
    budget: EmbeddingBudget,
    signal?: AbortSignal,
  ): Promise<readonly (readonly number[])[]>;
}

export interface RecallBudget {
  readonly maxCalls: number;
  readonly maxCandidates: number;
  readonly maxEpisodes: number;
  readonly maxSourceBytes: number;
  readonly maxContextTokens: number;
  readonly maxDurationMs: number;
}

export interface RetrievalReceipt {
  readonly schemaVersion: 1;
  readonly id: string;
  readonly snapshotId: string;
  readonly policyVersion: string;
  readonly retrievalVersion: string;
  readonly indexGeneration: number;
  readonly embeddingFingerprint: EmbeddingFingerprint | null;
  readonly mode: 'lexical' | 'hybrid';
  readonly hits: readonly {
    readonly chunkId: string;
    readonly episodeId: string;
    readonly spans: readonly SourceSpan[];
    readonly lexicalRank: number | null;
    readonly semanticRank: number | null;
    readonly fusionScore: number;
  }[];
  readonly elapsedMs: number;
  readonly sourceBytes: number;
  readonly contextTokens: number;
}

export interface MemoryItem {
  readonly schemaVersion: 1;
  readonly id: string;
  readonly snapshotId: string;
  readonly kind: 'episode' | 'reportedAssertion' | 'procedure' | 'styleExample';
  readonly text: string;
  readonly sourceSpans: readonly SourceSpan[];
  readonly episodeIds: readonly string[];
  readonly eventAt: string | null;
  readonly observedAt: string;
  readonly support: 'observed' | 'reported' | 'hypothesis' | 'unknown' | 'conflicting';
  readonly contradictionIds: readonly string[];
  readonly limitations: readonly string[];
  readonly extractionVersion: string;
  readonly policyVersion: string;
}

export interface SkillDefinition {
  readonly schemaVersion: 1;
  readonly id: string;
  readonly trigger: string;
  readonly inputs: readonly string[];
  readonly steps: readonly string[];
  readonly outcomeCheck: string;
  readonly support: 'observed' | 'proposed';
  readonly memoryItemIds: readonly string[];
  readonly sourceSpans: readonly SourceSpan[];
  readonly limitations: readonly string[];
}

export interface MemorySnapshot {
  readonly schemaVersion: 1;
  readonly id: string;
  readonly corpusSnapshotId: string;
  readonly itemIds: readonly string[];
  readonly extractionVersion: string;
  readonly createdAt: string;
}

export interface PersonaManifest {
  readonly schemaVersion: 1;
  readonly id: string;
  readonly personaId: string;
  readonly profileVersion: string;
  readonly skillIds: readonly string[];
  readonly exemplarIds: readonly string[];
  readonly corpusSnapshotId: string;
  readonly memorySnapshotId: string;
  readonly sourceRefs: readonly RevisionReference[];
  readonly splitVersion: string;
  readonly preparationVersion: string;
  readonly retrievalVersion: string;
  readonly promptVersion: string;
  readonly embeddingFingerprint: EmbeddingFingerprint | null;
  readonly mode: 'fixedContext' | 'agenticRecall';
  readonly recallBudget: RecallBudget;
  readonly modelSettings: readonly {
    readonly component: 'stalker' | 'person-fabric' | 'person' | 'brain-analyzator';
    readonly provider: 'codex' | 'kimi';
    readonly requestedModel: string | null;
    readonly requestedEffort: 'low' | 'medium' | 'high';
  }[];
  readonly createdAt: string;
}

export interface RecallResult {
  readonly items: readonly MemoryItem[];
  readonly receipt: RetrievalReceipt;
  readonly limitations: readonly string[];
}

// Native host binds persona, grants, snapshot, split and cumulative budgets when
// constructing the port. Model actions cannot supply or expand those scopes.
export interface MemoryPort {
  recall(
    query: string,
    intent: 'facts' | 'context' | 'style' | 'procedure',
    signal?: AbortSignal,
  ): Promise<RecallResult>;
  // IDs outside the host-bound snapshot/reference partition must be rejected.
  readEpisode(
    episodeId: string,
    signal?: AbortSignal,
  ): Promise<{
    readonly episodeId: string;
    readonly sources: readonly {
      readonly span: SourceSpan;
      readonly text: string;
    }[];
    readonly receipt: RetrievalReceipt;
  }>;
}

// Simulation output is a separate namespace; it is never original-author memory.
export interface SimulationMemoryEntry {
  readonly schemaVersion: 1;
  readonly id: string;
  readonly experimentId: string;
  readonly manifestId: string;
  readonly trialId: string;
  readonly role: 'user' | 'simulation';
  readonly text: string;
  readonly createdAt: string;
}
