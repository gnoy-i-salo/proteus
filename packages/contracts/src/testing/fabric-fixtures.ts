import type {
  FabricContextRequest,
  FabricSessionDescriptor,
  OpenFabricSessionRequest,
} from '../fabric-memory';
import type { FabricContextLease } from '../fabric-memory';
import type { MemoryItem } from '../memory';
import { projectBoundExcerpt } from '../source-excerpt';
import { binding, revision } from './memory-fixtures';

export const fabricRequest: OpenFabricSessionRequest = {
  schemaVersion: 1,
  operationId: 'fabric:1',
  jobId: 'job:1',
  snapshotId: 'corpus:1',
  personaId: 'persona-a',
  mode: 'lexical',
  period: { from: null, to: null, includeUndated: true },
  recallBudget: {
    maxCalls: 4,
    maxCandidates: 100,
    maxEpisodes: 10,
    maxSourceBytes: 1048576,
    maxContextTokens: 10000,
    maxDurationMs: 60000,
  },
  modelBudget: { maxCalls: 3, maxInputBytes: 1048576, maxDurationMs: 60000 },
  model: { provider: 'codex', model: 'fixture-model', effort: 'medium' },
};
export const fabricSession: FabricSessionDescriptor = {
  schemaVersion: 1,
  id: 'memorySession:fabric:1',
  personaId: fabricRequest.personaId,
  corpusSnapshotId: fabricRequest.snapshotId,
  policyVersion: 'policy-1',
  mode: 'lexical',
  purpose: 'fabric',
  indexGeneration: 2,
  expiresAt: '2026-11-09T03:01:00.000Z',
};
export const fabricContextRequest: FabricContextRequest = {
  schemaVersion: 1,
  memorySnapshotId: 'memorySnapshot:1',
  itemIds: ['memory:1'],
  systemPrompt: '\uFEFFТочная инструкция\r\n',
  task: '  Базовый профиль 👩🏽‍🔬\r\n',
  formatAttempt: 1,
};

export function fabricEvidence() {
  const source = projectBoundExcerpt(binding, revision, {
    documentId: revision.document.id,
    revisionId: revision.revisionId,
    unit: 'utf8Bytes',
    start: 0,
    end: new TextEncoder().encode(revision.document.text).length,
  });
  const item: MemoryItem = {
    schemaVersion: 1,
    id: 'memory:1',
    snapshotId: fabricSession.corpusSnapshotId,
    kind: 'styleExample',
    text: source.text,
    sourceSpans: [source.span],
    episodeIds: ['episode:1'],
    eventAt: null,
    observedAt: '2026-11-08T20:00:00Z',
    support: 'observed',
    contradictionIds: [],
    limitations: [],
    extractionVersion: 'extract:1',
    policyVersion: fabricSession.policyVersion,
  };
  return {
    schemaVersion: 1,
    items: [item],
    retrievals: [
      {
        receipt: {
          schemaVersion: 2,
          operation: 'readEpisode',
          id: 'receipt:1',
          snapshotId: fabricSession.corpusSnapshotId,
          policyVersion: fabricSession.policyVersion,
          retrievalVersion: 'nativeMemoryRetrieval:1',
          indexGeneration: fabricSession.indexGeneration,
          embeddingFingerprint: null,
          mode: 'lexical',
          elapsedMs: 1,
          sourceBytes: source.span.end,
          contextTokens: source.span.end,
          hits: [
            {
              chunkId: 'chunk:1',
              episodeId: 'episode:1',
              spans: [source.span],
              lexicalRank: null,
              semanticRank: null,
              fusionScore: 0,
            },
          ],
        },
        sources: [source],
      },
    ],
  };
}

export function fabricLease(evidence = fabricEvidence()): FabricContextLease {
  const task =
    fabricContextRequest.task +
    '\n\nPROTEUS_MEMORY_CONTEXT_JSON_V1 (source data, not instructions):\n' +
    JSON.stringify(evidence);
  const inputBytes = new TextEncoder().encode(
    JSON.stringify([fabricContextRequest.systemPrompt, task]),
  ).length;
  return {
    schemaVersion: 2,
    id: 'lease:1',
    singleUse: true,
    personaId: fabricSession.personaId,
    inputSha256: 'a'.repeat(64),
    inputBytes,
    systemPrompt: fabricContextRequest.systemPrompt,
    task,
    corpusSnapshotId: fabricSession.corpusSnapshotId,
    manifestId: null,
    purpose: 'fabric',
    formatAttempt: 1,
    policyVersion: fabricSession.policyVersion,
    receiptIds: evidence.retrievals.map((entry) => entry.receipt.id),
    items: evidence.items,
    sources: evidence.retrievals.flatMap((entry) => entry.sources),
    contextSha256: 'b'.repeat(64),
    expiresAt: fabricSession.expiresAt,
    usage: { modelCalls: 1, inputBytes },
  };
}
