import type { CorpusCoverage, RevisionReference, SourceExcerpt, SourceSpan } from './corpus';

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

// snapshotId always identifies the immutable corpus snapshot.
export interface RetrievalReceipt {
  readonly schemaVersion: 2;
  readonly operation: 'recall' | 'readEpisode';
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

// snapshotId is the immutable corpus snapshot backing the spans, not MemorySnapshot.id.
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
  readonly modelCallBudget: ModelCallBudget;
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
  readonly sources: readonly SourceExcerpt[];
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
    readonly sources: readonly SourceExcerpt[];
    readonly receipt: RetrievalReceipt;
  }>;
  // Host orchestration only. These methods are never dispatched from model actions.
  publishResearch(
    draft: ResearchPublicationDraft,
    signal?: AbortSignal,
  ): Promise<ResearchPublicationReceipt>;
  freezeManifest(draft: PersonaManifestDraft, signal?: AbortSignal): Promise<ManifestFreezeReceipt>;
  appendSimulation(
    draft: SimulationMemoryDraft,
    signal?: AbortSignal,
  ): Promise<SimulationMemoryEntry>;
  reserveModelContext(
    request: ModelContextRequest,
    signal?: AbortSignal,
  ): Promise<ModelContextLease>;
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

/** Limits every model attempt, including format repair; independent of retrieval calls. */
export interface ModelCallBudget {
  readonly maxCalls: number;
  readonly maxInputBytes: number;
  readonly maxDurationMs: number;
}

/** Draft IDs only correlate a publication batch; persisted IDs/times/policy come from native. */
export type MemoryItemDraft = Omit<
  MemoryItem,
  | 'schemaVersion'
  | 'id'
  | 'snapshotId'
  | 'observedAt'
  | 'contradictionIds'
  | 'extractionVersion'
  | 'policyVersion'
> & {
  readonly draftId: string;
  readonly contradictions: readonly (
    | { readonly kind: 'published'; readonly itemId: string }
    | { readonly kind: 'draft'; readonly draftId: string }
  )[];
};

export interface ResearchPublicationDraft {
  readonly schemaVersion: 1;
  readonly extractionVersion: string;
  readonly items: readonly MemoryItemDraft[];
  // Audit references only; native derives actual span coverage from consumed leases.
  readonly analyzedRefs: readonly RevisionReference[];
  readonly contextLeaseIds: readonly string[];
}

export interface ResearchPublicationReceipt {
  readonly schemaVersion: 1;
  readonly id: string;
  readonly snapshot: MemorySnapshot;
  readonly itemIds: readonly { readonly draftId: string; readonly itemId: string }[];
  readonly coverage: CorpusCoverage;
  readonly analyzedSpans: readonly SourceSpan[];
  readonly publishedAt: string;
}

export type SkillDefinitionDraft = Omit<SkillDefinition, 'schemaVersion' | 'id'> & {
  readonly draftId: string;
};

// The native bound session derives corpus/persona/source/split/index identity.
export interface PersonaManifestDraft extends Pick<
  PersonaManifest,
  | 'profileVersion'
  | 'exemplarIds'
  | 'memorySnapshotId'
  | 'preparationVersion'
  | 'promptVersion'
  | 'mode'
  | 'recallBudget'
  | 'modelCallBudget'
  | 'modelSettings'
> {
  readonly schemaVersion: 1;
  readonly skills: readonly SkillDefinitionDraft[];
}

export interface ManifestFreezeReceipt {
  readonly schemaVersion: 1;
  readonly manifest: PersonaManifest;
  readonly skills: readonly SkillDefinition[];
}

export type SimulationMemoryDraft = Omit<SimulationMemoryEntry, 'id' | 'createdAt'>;

export type ModelContextRequest = {
  readonly schemaVersion: 2;
  // Native appends authorized context to this exact host-authored base input.
  // Caller-supplied hashes cannot prove which source text reached the model.
  readonly systemPrompt: string;
  readonly task: string;
  readonly receiptIds: readonly string[];
  readonly memoryItemIds: readonly string[];
  readonly formatAttempt: 1 | 2;
} & (
  | { readonly purpose: 'research'; readonly manifestId: null }
  | { readonly purpose: 'personaReply'; readonly manifestId: string }
);

// Native assembles this input and revalidates its exact bytes at CLI dispatch.
// This receipt alone never grants source access or permission to invoke a model.
export interface ModelContextLease {
  readonly schemaVersion: 2;
  readonly id: string;
  readonly singleUse: true;
  readonly personaId: string;
  // SHA256 of UTF8 JSON.stringify([systemPrompt, task]); inputBytes uses the same framing.
  readonly inputSha256: string;
  readonly inputBytes: number;
  readonly systemPrompt: string;
  readonly task: string;
  readonly corpusSnapshotId: string;
  readonly manifestId: string | null;
  readonly purpose: 'research' | 'personaReply';
  readonly formatAttempt: 1 | 2;
  readonly policyVersion: string;
  readonly receiptIds: readonly string[];
  readonly items: readonly MemoryItem[];
  readonly sources: readonly SourceExcerpt[];
  readonly contextSha256: string;
  readonly expiresAt: string;
  readonly usage: {
    readonly modelCalls: number;
    readonly inputBytes: number;
  };
}

// Optional nested extension: absence is the existing non-RAG profile format.
export interface PersonaMemoryProfile {
  readonly schemaVersion: 1;
  readonly manifestId: string;
  readonly memorySnapshotId: string;
  readonly skills: readonly SkillDefinition[];
}

// Audit metadata is not an access grant or proof of comparable knowledge.
export interface ExperimentMemoryProtocol {
  readonly schemaVersion: 1;
  readonly mode: 'fixedContext' | 'agenticRecall';
  readonly participants: readonly {
    readonly participantId: 'a' | 'b' | 'neutral';
    readonly manifestId: string;
  }[];
  readonly neutralKnowledgeCondition: 'matchedContext' | 'matchedRecall' | 'unmatched';
  readonly limitations: readonly string[];
}

// The only model-proposed actions. Publication, leases and manifests are host commands.
export type ModelMemoryAction =
  | {
      readonly action: 'recall';
      readonly query: string;
      readonly intent: 'facts' | 'context' | 'style' | 'procedure';
    }
  | { readonly action: 'readEpisode'; readonly episodeId: string };
