import { describe, expect, it } from 'vitest';
import type {
  CorpusIndexReceipt,
  CorpusSearchHit,
  CorpusSearchPort,
  CorpusSearchQuery,
  CorpusSearchResult,
} from './corpus-search';
import {
  decodeCorpusIndexReceipt,
  decodeCorpusSearchQuery,
  decodeCorpusSearchResult,
} from './corpus-search-codec';

// Cyrillic, mixed scripts, combining marks, emoji, exact whitespace: preserved byte-exact.
const exactQueryText = '  «салo» и сало 👩🏽‍🔬 é\r\nисточник\t';
const emptySha256 = 'e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855';

const query: CorpusSearchQuery = {
  jobId: 'job:поиск-1',
  snapshotId: 'snapshot:1',
  personaId: 'persona:a',
  queryText: exactQueryText,
  period: { from: -8640000000000, to: 1760000000000, includeUndated: true },
  limit: 25,
  offset: 0,
  expectedIndexGeneration: null,
  maxDurationMs: 30000,
};

const hit: CorpusSearchHit = {
  chunkId: 'chunk:1',
  snapshotId: 'snapshot:1',
  indexGeneration: 7,
  bm25: -12.5,
  documentId: 'doc:1',
  revisionId: 'rev:1',
  spanStart: 0,
  spanEnd: 1000,
  personaId: 'persona:a',
  grantId: 'grant:1',
  bindingId: 'binding:1',
  periodFrom: 1700000000000,
  periodTo: 1700000000000,
  conversationId: 'conversation:1',
  threadId: null,
};

const secondHit: CorpusSearchHit = {
  ...hit,
  chunkId: 'chunk:2',
  bm25: 0.25,
  periodFrom: null,
  periodTo: null,
  conversationId: null,
  threadId: 'thread:9',
};

const result: CorpusSearchResult = {
  schemaVersion: 1,
  snapshotId: 'snapshot:1',
  snapshotGeneration: 3,
  indexGeneration: 7,
  hasMore: true,
  hits: [hit, secondHit],
};

const receipt: CorpusIndexReceipt = {
  schemaVersion: 1,
  snapshotId: 'snapshot:1',
  snapshotGeneration: 3,
  indexGeneration: 7,
  policyFingerprint: 'nativeFts:1:abc123',
  contentSha256: emptySha256,
  chunkCount: 2,
  createdAt: '1762588800',
};

describe('corpus search query boundary', () => {
  it('preserves exact Cyrillic, mixed, whitespace-heavy and Unicode query text', () => {
    expect(decodeCorpusSearchQuery(query)).toEqual(query);
    expect(decodeCorpusSearchQuery(query).queryText).toBe(exactQueryText);
  });
  it('accepts maximal byte, integer and duration budgets', () => {
    const maximal = {
      ...query,
      jobId: 'a'.repeat(128),
      snapshotId: 'b'.repeat(256),
      personaId: 'c'.repeat(256),
      queryText: 'я'.repeat(2048),
      limit: 100,
      offset: 10000,
      expectedIndexGeneration: Number.MAX_SAFE_INTEGER,
      maxDurationMs: 60000,
    };
    expect(decodeCorpusSearchQuery(maximal)).toEqual(maximal);
  });
  it('requires expectedIndexGeneration as soon as offset > 0', () => {
    expect(decodeCorpusSearchQuery({ ...query, offset: 1, expectedIndexGeneration: 7 })).toEqual({
      ...query,
      offset: 1,
      expectedIndexGeneration: 7,
    });
    expect(() => decodeCorpusSearchQuery({ ...query, offset: 1 })).toThrow();
    expect(() =>
      decodeCorpusSearchQuery({ ...query, offset: 10000, expectedIndexGeneration: null }),
    ).toThrow();
  });
  it.each([
    { queryText: '' },
    { queryText: 'я'.repeat(2048) + 'a' },
    { jobId: '' },
    { jobId: '   ' },
    { jobId: 'a'.repeat(129) },
    { jobId: 'job:\u0001' },
    { snapshotId: 'b'.repeat(257) },
    { personaId: '   ' },
    { personaId: 'c'.repeat(257) },
    { period: { from: 5, to: 4, includeUndated: true } },
    { period: { from: Number.MAX_SAFE_INTEGER + 1, to: null, includeUndated: true } },
    { period: { from: null, to: 1.5, includeUndated: true } },
    { period: { from: null, includeUndated: true } },
    { limit: 0 },
    { limit: 101 },
    { limit: 1.5 },
    { offset: -1 },
    { offset: 10001 },
    { offset: 0.5 },
    { maxDurationMs: 0 },
    { maxDurationMs: 60001 },
    { expectedIndexGeneration: 0 },
    { expectedIndexGeneration: -1 },
    { expectedIndexGeneration: Number.MAX_SAFE_INTEGER + 1 },
    { expectedIndexGeneration: 1.5 },
    { maxDurationMs: Number.POSITIVE_INFINITY },
    { grantIds: ['grant:1'] },
    { bindingIds: ['binding:1'] },
    { chunks: ['chunk:1'] },
    { text: 'исходный текст' },
    { sourcePath: '/tmp/private' },
    { jobRef: 'job:1' },
    {
      jobId: undefined,
    },
  ])('rejects invalid query shape %j', (change) => {
    expect(() => decodeCorpusSearchQuery({ ...query, ...change })).toThrow();
  });
  it('rejects a query missing the required-null expectedIndexGeneration key', () => {
    const missingExpected = {
      jobId: 'job:1',
      snapshotId: 'snapshot:1',
      personaId: 'persona:a',
      queryText: 'тест',
      period: { from: null, to: null, includeUndated: true },
      limit: 10,
      offset: 0,
      maxDurationMs: 1000,
    };
    expect(() => decodeCorpusSearchQuery(missingExpected)).toThrow();
  });
});

describe('corpus search result boundary', () => {
  it('decodes an exact native response with nullable fields and negative bm25', () => {
    expect(decodeCorpusSearchResult(result)).toEqual(result);
    expect(decodeCorpusSearchResult({ ...result, hits: [secondHit], hasMore: false })).toEqual({
      ...result,
      hits: [secondHit],
      hasMore: false,
    });
  });
  it('accepts maximal safe generations and span offsets', () => {
    const maximalHit = {
      ...hit,
      indexGeneration: Number.MAX_SAFE_INTEGER,
      spanStart: 0,
      spanEnd: 1_000_000_000_000,
      bm25: -0.0001,
    };
    expect(
      decodeCorpusSearchResult({
        ...result,
        snapshotGeneration: Number.MAX_SAFE_INTEGER,
        indexGeneration: Number.MAX_SAFE_INTEGER,
        hits: [maximalHit],
      }),
    ).toBeTruthy();
  });
  it('rejects a continuation flag on an empty page', () => {
    expect(() => decodeCorpusSearchResult({ ...result, hits: [] })).toThrow();
  });
  it('accepts empty and short final pages without a continuation flag', () => {
    expect(decodeCorpusSearchResult({ ...result, hits: [], hasMore: false })).toEqual({
      ...result,
      hits: [],
      hasMore: false,
    });
    expect(decodeCorpusSearchResult({ ...result, hits: [secondHit], hasMore: false })).toEqual({
      ...result,
      hits: [secondHit],
      hasMore: false,
    });
  });
  it('rejects hits from a foreign snapshot or index generation', () => {
    expect(() =>
      decodeCorpusSearchResult({ ...result, hits: [{ ...hit, snapshotId: 'snapshot:other' }] }),
    ).toThrow();
    expect(() =>
      decodeCorpusSearchResult({ ...result, hits: [{ ...hit, indexGeneration: 8 }] }),
    ).toThrow();
  });
  it('rejects duplicate chunk ids across hits', () => {
    expect(() =>
      decodeCorpusSearchResult({ ...result, hits: [hit, { ...hit, bm25: 1 }] }),
    ).toThrow();
  });
  it('rejects a hit missing the required-null conversationId key', () => {
    const missingNullable = {
      chunkId: 'chunk:1',
      snapshotId: 'snapshot:1',
      indexGeneration: 7,
      bm25: -1,
      documentId: 'doc:1',
      revisionId: 'rev:1',
      spanStart: 0,
      spanEnd: 1,
      personaId: 'persona:a',
      grantId: 'grant:1',
      bindingId: 'binding:1',
      periodFrom: null,
      periodTo: null,
      threadId: null,
    };
    expect(() => decodeCorpusSearchResult({ ...result, hits: [missingNullable] })).toThrow();
  });
  it.each([
    { schemaVersion: 2 },
    { schemaVersion: 0 },
    { schemaVersion: '1' },
    { snapshotGeneration: 0 },
    { snapshotGeneration: Number.MAX_SAFE_INTEGER + 1 },
    { snapshotGeneration: 2.5 },
    { indexGeneration: 0 },
    { hasMore: 'yes' },
    { hits: [] as unknown[], extra: true },
    { snapshotId: 'snapshot:foreign', hits: [hit] },
  ])('rejects invalid result shape %j', (change) => {
    expect(() => decodeCorpusSearchResult({ ...result, ...change })).toThrow();
  });
  it('caps hit pages at 100 entries', () => {
    const hits = Array.from({ length: 101 }, (_, index) => ({ ...hit, chunkId: `chunk:${index}` }));
    expect(() => decodeCorpusSearchResult({ ...result, hits })).toThrow();
    const page = hits.slice(0, 100);
    expect(decodeCorpusSearchResult({ ...result, hits: page }).hits).toHaveLength(100);
  });
  it.each([
    { bm25: Number.POSITIVE_INFINITY },
    { bm25: Number.NaN },
    { bm25: '1' },
    { spanStart: -1 },
    { spanStart: 1.5 },
    { spanEnd: 1_000_000_000_001 },
    { spanEnd: 0 },
    { spanStart: 500, spanEnd: 100 },
    { spanStart: 50, spanEnd: 50 },
    { periodFrom: Number.MAX_SAFE_INTEGER + 1 },
    { periodTo: Number.NEGATIVE_INFINITY },
    { periodFrom: 200, periodTo: 100 },
    { conversationId: '' },
    { conversationId: 7 },
    { threadId: 'x'.repeat(257) },
    { chunkId: '' },
    { episodeId: 'episode:1' },
    { text: 'исходный текст' },
    { sourcePath: '/tmp/private' },
    { personaId: 'persona:\u0002' },
  ])('rejects invalid hit shape %j', (change) => {
    expect(() => decodeCorpusSearchResult({ ...result, hits: [{ ...hit, ...change }] })).toThrow();
  });
});

describe('corpus index receipt boundary', () => {
  it('decodes the exact native receipt and preserves audit seconds digits', () => {
    expect(decodeCorpusIndexReceipt(receipt)).toEqual(receipt);
    expect(decodeCorpusIndexReceipt(receipt).createdAt).toBe('1762588800');
  });
  it('preserves the full native unsigned seconds range and rejects overflow', () => {
    const maximum = '18446744073709551615';
    expect(decodeCorpusIndexReceipt({ ...receipt, createdAt: maximum }).createdAt).toBe(maximum);
    expect(() =>
      decodeCorpusIndexReceipt({ ...receipt, createdAt: '18446744073709551616' }),
    ).toThrow();
  });
  it('accepts empty and maximal chunk manifests with maximal fingerprints', () => {
    expect(decodeCorpusIndexReceipt({ ...receipt, chunkCount: 0 })).toBeTruthy();
    expect(
      decodeCorpusIndexReceipt({
        ...receipt,
        chunkCount: 1_000_000,
        policyFingerprint: 'f'.repeat(256),
      }),
    ).toBeTruthy();
  });
  it.each([
    { schemaVersion: 2 },
    { schemaVersion: '1' },
    { snapshotGeneration: 0 },
    { snapshotGeneration: -3 },
    { snapshotGeneration: Number.MAX_SAFE_INTEGER + 1 },
    { indexGeneration: 0 },
    { policyFingerprint: '' },
    { policyFingerprint: 'f'.repeat(257) },
    { policyFingerprint: 'policy:\u0001' },
    { contentSha256: emptySha256.toUpperCase() },
    { contentSha256: emptySha256.slice(0, 63) },
    { contentSha256: `${emptySha256.slice(0, 63)}g` },
    { contentSha256: emptySha256.slice(0, 64) + 'a' },
    { chunkCount: -1 },
    { chunkCount: 1_000_001 },
    { chunkCount: 0.5 },
    { createdAt: '2026-11-08T12:00:00Z' },
    { createdAt: '1762588800.5' },
    { createdAt: 'abc' },
    { createdAt: '' },
    { createdAt: '1'.repeat(21) },
    { createdAt: -1 },
    { createdAt: null },
    { createdAt: '1762588800', snapshotGeneration: 1.5 },
  ])('rejects invalid receipt shape %j', (change) => {
    expect(() => decodeCorpusIndexReceipt({ ...receipt, ...change })).toThrow();
  });
  it('rejects a receipt missing the createdAt key entirely', () => {
    const missingCreatedAt = {
      schemaVersion: 1,
      snapshotId: 'snapshot:1',
      snapshotGeneration: 3,
      indexGeneration: 7,
      policyFingerprint: 'nativeFts:1:abc123',
      contentSha256: emptySha256,
      chunkCount: 2,
    };
    expect(() => decodeCorpusIndexReceipt(missingCreatedAt)).toThrow();
  });
});

describe('corpus search port', () => {
  it('exposes rebuild and search for the native adapter and later MemoryPort host', async () => {
    const port: CorpusSearchPort = {
      rebuild: () => Promise.resolve(receipt),
      search: () => Promise.resolve(result),
    };
    await expect(port.rebuild('job:1', 'snapshot:1', 60000)).resolves.toEqual(receipt);
    await expect(port.search(query)).resolves.toEqual(result);
  });
});

describe('duplicate JSON text keys', () => {
  it('cannot be detected by an object decoder after parsing', () => {
    // JSON.parse keeps the last duplicate text key before any object decoder
    // runs, so duplicate keys inside one JSON object are collapsed upstream and
    // can never be observed — or rejected — by these object decoders. This test
    // documents that real limitation instead of pretending detection.
    const parsed = JSON.parse('{"schemaVersion":1,"chunkCount":0,"chunkCount":1}') as {
      chunkCount: number;
    };
    expect(parsed.chunkCount).toBe(1);
  });
});
