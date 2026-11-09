import type { RecallBudget, RetrievalReceipt, EmbeddingFingerprint } from './memory';
import type { SourceSpan } from './corpus';
import { memoryFields as fields, memoryReceiptText as text } from './memory-wire-validation';

function integer(value: unknown, minimum = 0): number {
  if (!Number.isSafeInteger(value) || Number(value) < minimum)
    throw new Error('Invalid memory receipt integer.');
  return Number(value);
}

function sha256(value: unknown): string {
  const hash = text(value);
  if (!/^[a-f0-9]{64}$/u.test(hash)) throw new Error('Invalid embedding SHA256.');
  return hash;
}

export function decodeEmbeddingFingerprint(value: unknown): EmbeddingFingerprint | null {
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

function sourceSpan(value: unknown): SourceSpan {
  const input = fields(value, ['documentId', 'revisionId', 'unit', 'start', 'end']);
  const documentId = text(input.documentId);
  const revisionId = text(input.revisionId);
  const start = integer(input.start);
  const end = integer(input.end);
  if (input.unit !== 'utf8Bytes' || end <= start)
    throw new Error('Memory receipt span is outside the bound revisions.');
  return { documentId, revisionId, unit: 'utf8Bytes', start, end };
}

/** Structural decoding with host limits; this does not establish source membership or access. */
export function decodeRetrievalReceipt(value: unknown, budget: RecallBudget): RetrievalReceipt {
  const legacy =
    value !== null &&
    typeof value === 'object' &&
    'schemaVersion' in value &&
    value.schemaVersion === 1;
  const input = fields(value, [
    'schemaVersion',
    ...(legacy ? [] : ['operation']),
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
  // V1 only represented ranked recall: its validator required a non-null rank.
  // Upgrade that exact shape without rewriting the caller's retained payload.
  const operation = legacy ? 'recall' : input.operation;
  for (const limit of Object.values(budget)) integer(limit, 1);
  if (
    (input.schemaVersion !== 1 && input.schemaVersion !== 2) ||
    (operation !== 'recall' && operation !== 'readEpisode') ||
    (input.mode !== 'lexical' && input.mode !== 'hybrid')
  )
    throw new Error('Memory receipt version or bound scope mismatch.');
  const embeddingFingerprint = decodeEmbeddingFingerprint(input.embeddingFingerprint);
  if ((input.mode === 'hybrid') !== (embeddingFingerprint !== null))
    throw new Error('Memory receipt embedding space mismatch.');
  if (!Array.isArray(input.hits) || input.hits.length > budget.maxCandidates)
    throw new Error('Memory receipt candidate budget exceeded.');
  const sourceBytes = integer(input.sourceBytes);
  const contextTokens = integer(input.contextTokens);
  const elapsedMs = integer(input.elapsedMs);
  if (
    sourceBytes > budget.maxSourceBytes ||
    contextTokens > budget.maxContextTokens ||
    elapsedMs > budget.maxDurationMs
  )
    throw new Error('Memory receipt context budget exceeded.');
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
      episodes.size > budget.maxEpisodes ||
      !Array.isArray(hit.spans) ||
      !hit.spans.length ||
      hit.spans.length > budget.maxSourceBytes
    )
      throw new Error('Memory receipt episode or span budget exceeded.');
    const spans = hit.spans.map((span) => sourceSpan(span));
    if (spans.some((span) => span.end - span.start > sourceBytes))
      throw new Error('Memory receipt span exceeds reported source bytes.');
    const lexicalRank = hit.lexicalRank === null ? null : integer(hit.lexicalRank, 1);
    const semanticRank = hit.semanticRank === null ? null : integer(hit.semanticRank, 1);
    if (
      (operation === 'recall' && lexicalRank === null && semanticRank === null) ||
      (operation === 'readEpisode' &&
        (lexicalRank !== null || semanticRank !== null || hit.fusionScore !== 0)) ||
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
    schemaVersion: 2,
    operation,
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
