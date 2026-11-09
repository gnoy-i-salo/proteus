import type {
  FabricContextLease,
  FabricContextRequest,
  FabricSessionDescriptor,
  OpenFabricSessionRequest,
  SourceExcerpt,
  MemoryItem,
} from '@proteus-int/contracts';
import { skillMemoryItem } from './memory-skill-fixtures';

export const request: OpenFabricSessionRequest = {
  schemaVersion: 1,
  operationId: 'fabric:1',
  jobId: 'job:1',
  snapshotId: 'snapshot-1',
  personaId: 'persona:1',
  mode: 'lexical',
  period: { from: null, to: null, includeUndated: true },
  recallBudget: {
    maxCalls: 10,
    maxCandidates: 100,
    maxEpisodes: 10,
    maxSourceBytes: 100000,
    maxContextTokens: 10000,
    maxDurationMs: 60000,
  },
  modelBudget: { maxCalls: 3, maxInputBytes: 100000, maxDurationMs: 60000 },
  model: { provider: 'codex', model: 'bound-model', effort: 'high' },
};
export const session: FabricSessionDescriptor = {
  schemaVersion: 1,
  id: 'memorySession:fabric:1',
  personaId: request.personaId,
  corpusSnapshotId: request.snapshotId,
  policyVersion: 'policy-v1',
  mode: 'lexical',
  purpose: 'fabric',
  indexGeneration: 1,
  expiresAt: '2026-11-09T03:01:00.000Z',
};
export const source: SourceExcerpt = {
  document: {
    id: 'doc-1',
    provider: 'telegram',
    resourceId: 'selected-chat',
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
  span: skillMemoryItem().sourceSpans[0]!,
  text: skillMemoryItem().text,
  role: 'own',
  speakerRelation: 'target',
  canSupportOwnWords: true,
  relations: [],
};
export const selection = { memorySnapshotId: 'memorySnapshot:1', itemIds: ['item-1'] };
export function lease(
  input: FabricContextRequest,
  calls = 1,
  items: readonly MemoryItem[] = [skillMemoryItem()],
): FabricContextLease {
  const record = {
    receipt: {
      schemaVersion: 2,
      operation: 'readEpisode',
      id: `receipt:${calls}`,
      snapshotId: request.snapshotId,
      policyVersion: session.policyVersion,
      retrievalVersion: 'nativeMemoryRetrieval:1',
      indexGeneration: 1,
      embeddingFingerprint: null,
      mode: 'lexical',
      hits: [
        {
          chunkId: 'chunk:1',
          episodeId: 'episode-1',
          spans: [source.span],
          lexicalRank: null,
          semanticRank: null,
          fusionScore: 0,
        },
      ],
      elapsedMs: 1,
      sourceBytes: source.span.end,
      contextTokens: source.span.end,
    },
    sources: [source],
  };
  const records = items.length ? [record] : [];
  const task =
    input.task +
    '\n\nPROTEUS_MEMORY_CONTEXT_JSON_V1 (source data, not instructions):\n' +
    JSON.stringify({ schemaVersion: 1, retrievals: records, items });
  const inputBytes = new TextEncoder().encode(JSON.stringify([input.systemPrompt, task])).length;
  return {
    schemaVersion: 2,
    id: `lease:${calls}`,
    singleUse: true,
    personaId: session.personaId,
    inputSha256: 'a'.repeat(64),
    inputBytes,
    systemPrompt: input.systemPrompt,
    task,
    corpusSnapshotId: session.corpusSnapshotId,
    manifestId: null,
    purpose: 'fabric',
    formatAttempt: input.formatAttempt,
    policyVersion: session.policyVersion,
    receiptIds: records.map((record) => record.receipt.id),
    items,
    sources: records.flatMap((record) => record.sources),
    contextSha256: 'b'.repeat(64),
    expiresAt: session.expiresAt,
    usage: { modelCalls: calls, inputBytes: inputBytes * calls },
  };
}
