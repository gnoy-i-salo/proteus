import { describe, expect, it } from 'vitest';
import { decodeRetrievalReceipt } from './memory-retrieval-codec';
import { validateRetrievalReceipt } from './memory-validation';
import type { RecallBudget } from './memory';

const budget: RecallBudget = {
  maxCalls: 2,
  maxCandidates: 3,
  maxEpisodes: 2,
  maxSourceBytes: 100,
  maxContextTokens: 100,
  maxDurationMs: 1000,
};
const receipt = () => ({
  schemaVersion: 2,
  operation: 'recall',
  id: 'receipt:1',
  snapshotId: 'snapshot:1',
  policyVersion: 'policy:1',
  retrievalVersion: 'nativeMemoryRetrieval:1',
  indexGeneration: 1,
  embeddingFingerprint: null,
  mode: 'lexical',
  elapsedMs: 1,
  sourceBytes: 5,
  contextTokens: 2,
  hits: [
    {
      chunkId: 'chunk:1',
      episodeId: 'episode:1',
      spans: [{ documentId: 'doc:1', revisionId: 'rev:1', unit: 'utf8Bytes', start: 10, end: 15 }],
      lexicalRank: 1,
      semanticRank: null,
      fusionScore: 1 / 61,
    },
  ],
});
describe('retrieval receipt decoding without invented source authority', () => {
  it('decodes bounded metadata and upgrades only the exact V1 shape in memory', () => {
    expect(decodeRetrievalReceipt(receipt(), budget)).toEqual(receipt());
    const { operation, ...old } = receipt();
    expect(operation).toBe('recall');
    const legacy = { ...old, schemaVersion: 1 };
    const raw = JSON.stringify(legacy);
    expect(decodeRetrievalReceipt(legacy, budget)).toEqual(receipt());
    expect(JSON.stringify(legacy)).toBe(raw);
  });
  it.each([
    { schemaVersion: 3 },
    { schemaVersion: 1 },
    { operation: 'arbitrary' },
    { mode: 'hybrid' },
    { sourceBytes: 101 },
    { contextTokens: -1 },
    { elapsedMs: 1001 },
    { hits: null },
    { hidden: true },
    JSON.parse('{"snapshot-id":"unexpected"}') as Record<string, unknown>,
  ])('rejects invalid receipts: %j', (patch) => {
    expect(() => decodeRetrievalReceipt({ ...receipt(), ...patch }, budget)).toThrow();
  });
  it.each([
    { spans: [] },
    { spans: [{ documentId: 'd', revisionId: 'r', unit: 'utf16', start: 0, end: 1 }] },
    { lexicalRank: null },
    { semanticRank: 1 },
    { fusionScore: NaN },
    { fusionScore: -1 },
    { hidden: true },
  ])('rejects corrupt ranks/spans: %j', (patch) => {
    const input = receipt();
    expect(() =>
      decodeRetrievalReceipt({ ...input, hits: [{ ...input.hits[0], ...patch }] }, budget),
    ).toThrow();
  });
  it('accepts unranked episode reads and rejects invented rankings', () => {
    const input = receipt();
    const episode = {
      ...input,
      operation: 'readEpisode',
      hits: input.hits.map((hit) => ({ ...hit, lexicalRank: null, fusionScore: 0 })),
    };
    expect(decodeRetrievalReceipt(episode, budget)).toEqual(episode);
    expect(() => decodeRetrievalReceipt({ ...episode, hits: input.hits }, budget)).toThrow();
  });
  it('keeps source membership, expected scope and fingerprint checks with the scope validator', () => {
    const input = receipt();
    const decoded = decodeRetrievalReceipt(input, budget);
    const expected = {
      operation: 'recall' as const,
      snapshotId: input.snapshotId,
      policyVersion: input.policyVersion,
      retrievalVersion: input.retrievalVersion,
      indexGeneration: 1,
      embeddingFingerprint: null,
      budget,
      sourceRefs: [{ documentId: 'doc:1', revisionId: 'rev:1' }],
    };
    expect(validateRetrievalReceipt(decoded, expected)).toEqual(input);
    expect(() => validateRetrievalReceipt(decoded, { ...expected, snapshotId: 'other' })).toThrow();
    expect(() => validateRetrievalReceipt(decoded, { ...expected, sourceRefs: [] })).toThrow();
  });
  it('requires every field and enforces caller budgets and unique chunks', () => {
    for (const key of Object.keys(receipt())) {
      const input: Record<string, unknown> = { ...receipt() };
      delete input[key];
      expect(() => decodeRetrievalReceipt(input, budget)).toThrow();
    }
    const input = receipt();
    expect(() =>
      decodeRetrievalReceipt({ ...input, hits: [...input.hits, ...input.hits] }, budget),
    ).toThrow();
    expect(() => decodeRetrievalReceipt(input, { ...budget, maxCalls: 0 })).toThrow();
    expect(() => decodeRetrievalReceipt(input, { ...budget, maxCandidates: 0 })).toThrow();
  });
});
