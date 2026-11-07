import { describe, expect, it } from 'vitest';
import { parseMemoryAction, validateRetrievalReceipt } from './memory-validation';
import type { RetrievalReceiptScope } from './memory-validation';
import type { EmbeddingFingerprint, RetrievalReceipt } from './memory';

const span = { documentId: 'd1', revisionId: 'r1', unit: 'utf8Bytes' as const, start: 0, end: 12 };
const scope: RetrievalReceiptScope = {
  snapshotId: 'corpus-1',
  policyVersion: 'policy-1',
  retrievalVersion: 'rrf-1',
  indexGeneration: 2,
  sourceRefs: [{ documentId: 'd1', revisionId: 'r1' }],
  embeddingFingerprint: null,
  budget: {
    maxCalls: 4,
    maxCandidates: 20,
    maxEpisodes: 2,
    maxSourceBytes: 1000,
    maxContextTokens: 100,
    maxDurationMs: 10000,
  },
};
const receipt: RetrievalReceipt = {
  schemaVersion: 1,
  id: 'native-receipt',
  snapshotId: 'corpus-1',
  policyVersion: 'policy-1',
  retrievalVersion: 'rrf-1',
  indexGeneration: 2,
  embeddingFingerprint: null,
  mode: 'lexical',
  hits: [
    {
      chunkId: 'chunk-1',
      episodeId: 'episode-1',
      spans: [span],
      lexicalRank: 1,
      semanticRank: null,
      fusionScore: 1 / 61,
    },
  ],
  elapsedMs: 3,
  sourceBytes: 12,
  contextTokens: 4,
};

describe('structural retrieval receipt validation', () => {
  it('accepts bounded lexical evidence without inventing embedding metadata', () => {
    expect(validateRetrievalReceipt(receipt, scope)).toEqual(receipt);
    expect(
      validateRetrievalReceipt({ ...receipt, hits: [], sourceBytes: 0, contextTokens: 0 }, scope)
        .hits,
    ).toEqual([]);
  });
  it.each([
    { schemaVersion: 2 },
    { snapshotId: 'other-corpus' },
    { policyVersion: 'revoked-policy' },
    { retrievalVersion: 'other-algorithm' },
    { indexGeneration: 3 },
    { mode: 'other' },
    { sourceBytes: 1001 },
    { contextTokens: 101 },
    { elapsedMs: 10001 },
    { sourceBytes: -1 },
    { contextTokens: 1.2 },
    { mode: 'hybrid' },
    { 'snapshot-id': 'corpus-1' },
    { grantId: 'must-not-leak' },
  ])('rejects unknown schema, stale scope and invalid budgets: %j', (changes) => {
    expect(() => validateRetrievalReceipt({ ...receipt, ...changes }, scope)).toThrow();
  });
  it.each([
    { spans: [{ ...span, documentId: 'foreign' }] },
    { spans: [{ ...span, revisionId: 'foreign' }] },
    { spans: [{ ...span, unit: 'utf16' }] },
    { spans: [{ ...span, start: 13 }] },
    { spans: [] },
    { fusionScore: Number.NaN },
    { lexicalRank: 0 },
    { lexicalRank: null },
    { semanticRank: 1 },
    { token: 'must-not-leak' },
  ])('rejects unsupported or malformed hit provenance: %j', (changes) => {
    expect(() =>
      validateRetrievalReceipt({ ...receipt, hits: [{ ...receipt.hits[0], ...changes }] }, scope),
    ).toThrow();
  });
  it('rejects duplicate chunks and a candidate or episode count outside the bound session', () => {
    expect(() =>
      validateRetrievalReceipt({ ...receipt, hits: [receipt.hits[0], receipt.hits[0]] }, scope),
    ).toThrow();
    expect(() =>
      validateRetrievalReceipt(receipt, {
        ...scope,
        budget: { ...scope.budget, maxCandidates: 0 },
      }),
    ).toThrow();
    const hits = [1, 2, 3].map((n) => ({
      ...receipt.hits[0]!,
      chunkId: `c${n}`,
      episodeId: `e${n}`,
    }));
    expect(() => validateRetrievalReceipt({ ...receipt, hits }, scope)).toThrow();
  });
  it('checks the complete embedding fingerprint for hybrid results without mixing spaces', () => {
    const fingerprint: EmbeddingFingerprint = {
      modelId: 'local-test-model',
      modelRevision: 'revision-1',
      modelSha256: 'a'.repeat(64),
      tokenizerSha256: 'b'.repeat(64),
      dimensions: 3,
      pooling: 'mean',
      normalization: 'l2',
      queryTemplate: 'query: ',
      passageTemplate: 'passage: ',
      chunkingVersion: 'chunk-1',
    };
    const expected = { ...scope, embeddingFingerprint: fingerprint };
    const hybrid = {
      ...receipt,
      mode: 'hybrid',
      embeddingFingerprint: fingerprint,
      hits: [{ ...receipt.hits[0], semanticRank: 2 }],
    };
    expect(validateRetrievalReceipt(hybrid, expected).embeddingFingerprint).toEqual(fingerprint);
    expect(
      validateRetrievalReceipt(
        { ...hybrid, hits: [{ ...hybrid.hits[0], lexicalRank: null }] },
        expected,
      ).hits[0]?.semanticRank,
    ).toBe(2);
    expect(() =>
      validateRetrievalReceipt(
        { ...hybrid, embeddingFingerprint: { ...fingerprint, queryTemplate: 'changed: ' } },
        expected,
      ),
    ).toThrow();
    expect(() =>
      validateRetrievalReceipt(
        { ...hybrid, embeddingFingerprint: { ...fingerprint, dimensions: 0 } },
        expected,
      ),
    ).toThrow();
  });
});

it('does not confuse candidate count with source spans within one candidate', () => {
  const hits = [
    { ...receipt.hits[0]!, spans: [0, 1, 2].map((start) => ({ ...span, start, end: start + 1 })) },
  ];
  expect(
    validateRetrievalReceipt(
      { ...receipt, hits },
      { ...scope, budget: { ...scope.budget, maxCandidates: 1 } },
    ).hits,
  ).toEqual(hits);
});

describe('model memory action allowlist', () => {
  it.each([
    { action: 'recall', query: '  Анна → Борис\r\n', intent: 'context' },
    { action: 'readEpisode', episodeId: 'episode-1' },
  ])('preserves exact permitted action text %j', (value) => {
    expect(parseMemoryAction(value)).toEqual(value);
  });
  it.each([
    { action: 'publishResearch', items: [] },
    { action: 'freezeManifest' },
    { action: 'reserveModelContext' },
    { action: 'appendSimulation', text: 'Not original source' },
    { action: 'recall', query: 'x', intent: 'facts', grantId: 'foreign' },
    { action: 'recall', query: 'x', intent: 'facts', snapshotId: 'foreign' },
    { action: 'recall', query: 'x', intent: 'command' },
    { action: 'recall', query: ' ', intent: 'facts' },
    { action: 'recall', query: 'я'.repeat(2049), intent: 'facts' },
    { action: 'readEpisode', episodeId: '' },
    { action: 'readEpisode', episodeId: 'x', sql: 'SELECT secret' },
  ])('rejects host commands, widened scope and malformed model input %j', (value) => {
    expect(() => parseMemoryAction(value)).toThrow();
  });
});
