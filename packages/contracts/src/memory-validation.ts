import type {
  RecallBudget,
  RetrievalReceipt,
  EmbeddingFingerprint,
  ModelMemoryAction,
} from './memory';
import type { RevisionReference, SourceSpan } from './corpus';

export interface RetrievalReceiptScope {
  readonly snapshotId: string;
  readonly policyVersion: string;
  readonly retrievalVersion: string;
  readonly indexGeneration: number;
  readonly sourceRefs: readonly RevisionReference[];
  readonly embeddingFingerprint: EmbeddingFingerprint | null;
  readonly budget: RecallBudget;
}

function fields(value: unknown, expected: readonly string[]): Record<string, unknown> {
  if (!value || typeof value !== 'object' || Array.isArray(value))
    throw new Error('Invalid memory receipt object.');
  const keys = Object.keys(value);
  if (keys.length !== expected.length || keys.some((key) => !expected.includes(key)))
    throw new Error('Unknown, missing or mixed memory receipt fields.');
  return value as Record<string, unknown>;
}

function integer(value: unknown, minimum = 0): number {
  if (!Number.isSafeInteger(value) || Number(value) < minimum)
    throw new Error('Invalid memory receipt integer.');
  return Number(value);
}

function text(value: unknown, allowEmpty = false): string {
  if (typeof value !== 'string' || (!allowEmpty && !value.trim()) || value.length > 2048)
    throw new Error('Invalid memory receipt text.');
  return value;
}

function sha256(value: unknown): string {
  const hash = text(value);
  if (!/^[a-f0-9]{64}$/u.test(hash)) throw new Error('Invalid embedding SHA256.');
  return hash;
}

function fingerprint(value: unknown): EmbeddingFingerprint | null {
  if (value === null) return null;
  const input = fields(value, [
    'modelId',
    'modelRevision',
    'modelSha256',
    'tokenizerSha256',
    'dimensions',
    'pooling',
    'normalization',
    'queryTemplate',
    'passageTemplate',
    'chunkingVersion',
  ]);
  if (input.pooling !== 'mean' && input.pooling !== 'cls')
    throw new Error('Invalid embedding pooling.');
  if (input.normalization !== 'l2' && input.normalization !== 'none')
    throw new Error('Invalid embedding normalization.');
  return {
    modelId: text(input.modelId),
    modelRevision: text(input.modelRevision),
    modelSha256: sha256(input.modelSha256),
    tokenizerSha256: sha256(input.tokenizerSha256),
    dimensions: integer(input.dimensions, 1),
    pooling: input.pooling,
    normalization: input.normalization,
    queryTemplate: text(input.queryTemplate, true),
    passageTemplate: text(input.passageTemplate, true),
    chunkingVersion: text(input.chunkingVersion),
  };
}

function sourceSpan(value: unknown, allowed: ReadonlySet<string>): SourceSpan {
  const input = fields(value, ['documentId', 'revisionId', 'unit', 'start', 'end']);
  const documentId = text(input.documentId);
  const revisionId = text(input.revisionId);
  const start = integer(input.start);
  const end = integer(input.end);
  if (
    input.unit !== 'utf8Bytes' ||
    end <= start ||
    !allowed.has(JSON.stringify([documentId, revisionId]))
  )
    throw new Error('Memory receipt span is outside the bound revisions.');
  return { documentId, revisionId, unit: 'utf8Bytes', start, end };
}

/**
 * Structural validation against the expected bound session. A receipt is not an
 * access grant; native must recheck policy, expiry, cumulative usage and originals.
 */
export function validateRetrievalReceipt(
  value: unknown,
  scope: RetrievalReceiptScope,
): RetrievalReceipt {
  const input = fields(value, [
    'schemaVersion',
    'id',
    'snapshotId',
    'policyVersion',
    'retrievalVersion',
    'indexGeneration',
    'embeddingFingerprint',
    'mode',
    'hits',
    'elapsedMs',
    'sourceBytes',
    'contextTokens',
  ]);
  for (const limit of Object.values(scope.budget)) integer(limit, 1);
  if (
    input.schemaVersion !== 1 ||
    input.snapshotId !== scope.snapshotId ||
    input.policyVersion !== scope.policyVersion ||
    input.retrievalVersion !== scope.retrievalVersion ||
    input.indexGeneration !== scope.indexGeneration ||
    (input.mode !== 'lexical' && input.mode !== 'hybrid')
  )
    throw new Error('Memory receipt version or bound scope mismatch.');
  const embeddingFingerprint = fingerprint(input.embeddingFingerprint);
  if (
    JSON.stringify(embeddingFingerprint) !==
      JSON.stringify(fingerprint(scope.embeddingFingerprint)) ||
    (input.mode === 'hybrid') !== (embeddingFingerprint !== null)
  )
    throw new Error('Memory receipt embedding space mismatch.');
  if (!Array.isArray(input.hits) || input.hits.length > scope.budget.maxCandidates)
    throw new Error('Memory receipt candidate budget exceeded.');
  const sourceBytes = integer(input.sourceBytes);
  const contextTokens = integer(input.contextTokens);
  const elapsedMs = integer(input.elapsedMs);
  if (
    sourceBytes > scope.budget.maxSourceBytes ||
    contextTokens > scope.budget.maxContextTokens ||
    elapsedMs > scope.budget.maxDurationMs
  )
    throw new Error('Memory receipt context budget exceeded.');
  const allowed = new Set(
    scope.sourceRefs.map((ref) => JSON.stringify([ref.documentId, ref.revisionId])),
  );
  const chunks = new Set<string>();
  const episodes = new Set<string>();
  const hits = input.hits.map((value): RetrievalReceipt['hits'][number] => {
    const hit = fields(value, [
      'chunkId',
      'episodeId',
      'spans',
      'lexicalRank',
      'semanticRank',
      'fusionScore',
    ]);
    const chunkId = text(hit.chunkId);
    const episodeId = text(hit.episodeId);
    if (chunks.has(chunkId)) throw new Error('Duplicate memory receipt chunk.');
    chunks.add(chunkId);
    episodes.add(episodeId);
    if (
      episodes.size > scope.budget.maxEpisodes ||
      !Array.isArray(hit.spans) ||
      !hit.spans.length ||
      hit.spans.length > scope.budget.maxSourceBytes
    )
      throw new Error('Memory receipt episode or span budget exceeded.');
    const spans = hit.spans.map((span) => sourceSpan(span, allowed));
    if (spans.some((span) => span.end - span.start > sourceBytes))
      throw new Error('Memory receipt span exceeds reported source bytes.');
    const lexicalRank = hit.lexicalRank === null ? null : integer(hit.lexicalRank, 1);
    const semanticRank = hit.semanticRank === null ? null : integer(hit.semanticRank, 1);
    if (
      (lexicalRank === null && semanticRank === null) ||
      (input.mode === 'lexical' && semanticRank !== null)
    )
      throw new Error('Invalid memory receipt retrieval ranks.');
    if (
      typeof hit.fusionScore !== 'number' ||
      !Number.isFinite(hit.fusionScore) ||
      hit.fusionScore < 0
    )
      throw new Error('Invalid memory receipt fusion score.');
    return { chunkId, episodeId, spans, lexicalRank, semanticRank, fusionScore: hit.fusionScore };
  });
  return {
    schemaVersion: 1,
    id: text(input.id),
    snapshotId: text(input.snapshotId),
    policyVersion: text(input.policyVersion),
    retrievalVersion: text(input.retrievalVersion),
    indexGeneration: integer(input.indexGeneration),
    embeddingFingerprint,
    mode: input.mode,
    hits,
    elapsedMs,
    sourceBytes,
    contextTokens,
  };
}

/** Model action schema only; native still authorizes every call in its bound session. */
export function parseMemoryAction(value: unknown): ModelMemoryAction {
  const action = value && typeof value === 'object' && 'action' in value ? value.action : undefined;
  if (action === 'recall') {
    const input = fields(value, ['action', 'query', 'intent']);
    if (
      typeof input.query !== 'string' ||
      !input.query.trim() ||
      new TextEncoder().encode(input.query).length > 4096
    )
      throw new Error('Memory recall query exceeds the nonempty 4096-byte tool limit.');
    if (
      input.intent !== 'facts' &&
      input.intent !== 'context' &&
      input.intent !== 'style' &&
      input.intent !== 'procedure'
    )
      throw new Error('Invalid memory recall intent.');
    return { action: 'recall', query: input.query, intent: input.intent };
  }
  if (action === 'readEpisode') {
    const input = fields(value, ['action', 'episodeId']);
    const episodeId = text(input.episodeId);
    if (episodeId.length > 256) throw new Error('Invalid memory episode ID.');
    return { action: 'readEpisode', episodeId };
  }
  throw new Error('Only recall and readEpisode are model memory tools.');
}
