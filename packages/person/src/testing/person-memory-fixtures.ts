import type {
  OpenPersonaSessionRequest,
  PersonaSessionDescriptor,
  PersonProfile,
  PersonaContextLease,
  ModelContextRequest,
  RecallResult,
  MemoryItem,
} from '@proteus-int/contracts';
export const request: OpenPersonaSessionRequest = {
  schemaVersion: 1,
  operationId: 'person:1',
  jobId: 'job:1',
  personaId: 'persona:1',
  manifestId: 'manifest:1',
  period: { from: null, to: null, includeUndated: true },
};
export function session(): PersonaSessionDescriptor {
  return {
    schemaVersion: 1,
    id: 'session:1',
    personaId: request.personaId,
    corpusSnapshotId: 'corpus:1',
    policyVersion: 'policy:1',
    mode: 'lexical',
    purpose: 'personaReply',
    indexGeneration: 1,
    expiresAt: new Date(Date.now() + 60000).toISOString(),
    manifest: {
      schemaVersion: 1,
      id: request.manifestId,
      personaId: request.personaId,
      profileVersion: 'person-memory-profile:1',
      skillIds: [],
      exemplarIds: [],
      corpusSnapshotId: 'corpus:1',
      memorySnapshotId: 'memory:1',
      sourceRefs: [],
      splitVersion: 'split:1',
      preparationVersion: 'nativePreparation:1:' + 'a'.repeat(64),
      retrievalVersion: 'nativeMemoryRetrieval:1',
      promptVersion: 'person-fabric-memory:1',
      embeddingFingerprint: null,
      mode: 'agenticRecall',
      recallBudget: {
        maxCalls: 10,
        maxCandidates: 100,
        maxEpisodes: 10,
        maxSourceBytes: 100000,
        maxContextTokens: 10000,
        maxDurationMs: 60000,
      },
      modelCallBudget: { maxCalls: 8, maxInputBytes: 1000000, maxDurationMs: 60000 },
      modelSettings: (['stalker', 'person-fabric', 'person', 'brain-analyzator'] as const).map(
        (component) => ({
          component,
          provider: 'codex',
          requestedModel: 'pinned-model',
          requestedEffort: 'high',
        }),
      ),
      createdAt: '2026-11-09T00:00:00.000Z',
    },
  };
}
export const profile: PersonProfile = {
  id: request.personaId,
  label: 'Анна → Борис',
  palette: 'turquoise',
  archetype: 'Сведения ограничены',
  skills: [],
  rules: [],
  taskSkills: [],
  sourceStatus: 'synthetic',
  sourceContext: true,
  memory: {
    schemaVersion: 1,
    manifestId: request.manifestId,
    memorySnapshotId: 'memory:1',
    skills: [],
  },
};
export function retrieval(id = 'receipt:1'): RecallResult {
  return {
    items: [],
    limitations: [],
    sources: [],
    receipt: {
      schemaVersion: 2,
      operation: 'recall',
      id,
      snapshotId: 'corpus:1',
      policyVersion: 'policy:1',
      retrievalVersion: 'nativeMemoryRetrieval:1',
      indexGeneration: 1,
      embeddingFingerprint: null,
      mode: 'lexical',
      hits: [],
      elapsedMs: 1,
      sourceBytes: 0,
      contextTokens: 0,
    },
  };
}
export function lease(
  input: ModelContextRequest,
  bound: PersonaSessionDescriptor,
  calls = 1,
  records: readonly RecallResult[] = [],
  items: readonly MemoryItem[] = [],
  baseline: readonly RecallResult[] = [],
): PersonaContextLease {
  const selected = [
    ...baseline,
    ...records.filter((r) => input.receiptIds.includes(r.receipt.id)),
  ].map(({ receipt, sources }) => ({ receipt, sources }));
  const task =
    input.task +
    '\n\nPROTEUS_MEMORY_CONTEXT_JSON_V1 (source data, not instructions):\n' +
    JSON.stringify({ schemaVersion: 1, retrievals: selected, items });
  const inputBytes = new TextEncoder().encode(JSON.stringify([input.systemPrompt, task])).length;
  return {
    schemaVersion: 2,
    id: `lease:${calls}`,
    singleUse: true,
    personaId: bound.personaId,
    inputSha256: 'a'.repeat(64),
    inputBytes,
    systemPrompt: input.systemPrompt,
    task,
    corpusSnapshotId: bound.corpusSnapshotId,
    manifestId: bound.manifest.id,
    purpose: 'personaReply',
    formatAttempt: input.formatAttempt,
    policyVersion: bound.policyVersion,
    receiptIds: selected.map((record) => record.receipt.id),
    items,
    sources: selected.flatMap((r) => r.sources),
    contextSha256: 'b'.repeat(64),
    expiresAt: bound.expiresAt,
    usage: { modelCalls: calls, inputBytes: inputBytes * calls },
  };
}

export function sourceRetrieval(
  id = 'receipt:source',
  operation: 'recall' | 'readEpisode' = 'recall',
): RecallResult {
  const text = '  Анна → Борис\r\n**да** é 👩🏽‍🔬  ';
  const bytes = new TextEncoder().encode(text).length;
  const span = {
    documentId: 'doc:1',
    revisionId: 'revision:1',
    start: 0,
    end: bytes,
    unit: 'utf8Bytes' as const,
  };
  return {
    ...retrieval(id),
    sources: [
      {
        document: {
          id: 'doc:1',
          provider: 'telegram',
          resourceId: 'chat:1',
          authorId: 'ann',
          authorLabel: 'Анна',
          authorship: 'owner',
          kind: 'message',
          mode: 'dialogue',
          sourceUrl: 'https://example.org/ann',
          createdAt: '2026-06-01T00:00:00Z',
          retrievedAt: '2026-06-02T00:00:00Z',
          contentHash: 'a'.repeat(64),
          replyTo: 'boris-message',
          isForwarded: false,
          truncated: false,
          partition: 'reference',
        },
        span,
        text,
        role: 'own',
        speakerRelation: 'target',
        canSupportOwnWords: true,
        relations: [],
      },
    ],
    receipt: {
      ...retrieval(id).receipt,
      operation,
      hits: [
        {
          chunkId: 'chunk:1',
          episodeId: 'episode:1',
          spans: [span],
          lexicalRank: operation === 'recall' ? 1 : null,
          semanticRank: null,
          fusionScore: operation === 'recall' ? 1 / 61 : 0,
        },
      ],
      sourceBytes: bytes,
      contextTokens: bytes,
    },
  };
}
