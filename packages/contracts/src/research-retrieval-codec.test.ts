import { describe, expect, it } from 'vitest';
import { decodeResearchRetrieval } from './research-retrieval-codec';
import { projectBoundExcerpt } from './source-excerpt';
import { binding, revision } from './testing/memory-fixtures';
import type { ResearchSessionDescriptor } from './research-memory';
import type { RecallBudget } from './memory';

const session: ResearchSessionDescriptor = {
  schemaVersion: 1,
  id: 'session:1',
  personaId: binding.personaId,
  corpusSnapshotId: 'snapshot:1',
  policyVersion: binding.policyVersion,
  mode: 'lexical',
  indexGeneration: 2,
  expiresAt: '2026-11-09T03:01:00.000Z',
};
const budget: RecallBudget = {
  maxCalls: 3,
  maxCandidates: 100,
  maxEpisodes: 10,
  maxSourceBytes: 1048576,
  maxContextTokens: 10000,
  maxDurationMs: 60000,
};
function response() {
  const span = {
    documentId: revision.document.id,
    revisionId: revision.revisionId,
    unit: 'utf8Bytes' as const,
    start: 0,
    end: new TextEncoder().encode(revision.document.text).length,
  };
  return {
    receipt: {
      schemaVersion: 2,
      operation: 'recall',
      id: 'receipt:1',
      snapshotId: session.corpusSnapshotId,
      policyVersion: session.policyVersion,
      retrievalVersion: 'nativeMemoryRetrieval:1',
      indexGeneration: 2,
      embeddingFingerprint: null,
      mode: 'lexical',
      elapsedMs: 1,
      sourceBytes: span.end,
      contextTokens: 20,
      hits: [
        {
          chunkId: 'chunk:1',
          episodeId: 'episode:1',
          spans: [span],
          lexicalRank: 1,
          semanticRank: null,
          fusionScore: 1 / 61,
        },
      ],
    },
    sources: [projectBoundExcerpt(binding, revision, span)],
  };
}
describe('native research retrieval response', () => {
  it('retains exact original excerpts in native receipt order without fabricating memory items', () => {
    const input = response();
    expect(decodeResearchRetrieval(input, session, budget, { operation: 'recall' })).toEqual(input);
    expect(
      decodeResearchRetrieval(input, session, budget, { operation: 'recall' }),
    ).not.toHaveProperty('items');
  });
  it.each([
    { snapshotId: 'foreign' },
    { policyVersion: 'foreign' },
    { indexGeneration: 3 },
    { retrievalVersion: 'other' },
    { sourceBytes: 1000 },
    { schemaVersion: 1 },
  ])('rejects mismatched native receipt metadata: %j', (patch) => {
    const input = response();
    expect(() =>
      decodeResearchRetrieval(
        { ...input, receipt: { ...input.receipt, ...patch } },
        session,
        budget,
        { operation: 'recall' },
      ),
    ).toThrow();
  });
  it('does not upgrade legacy receipts inside a new live IPC response', () => {
    const input = response();
    const { operation, ...old } = input.receipt;
    expect(operation).toBe('recall');
    expect(() =>
      decodeResearchRetrieval(
        { ...input, receipt: { ...old, schemaVersion: 1 } },
        session,
        budget,
        { operation: 'recall' },
      ),
    ).toThrow();
  });
  it('rejects omitted, extra, foreign or reordered excerpts and unknown response fields', () => {
    const input = response();
    const source = input.sources[0]!;
    for (const sources of [
      [],
      [source, source],
      [{ ...source, span: { ...source.span, revisionId: 'foreign' } }],
    ])
      expect(() =>
        decodeResearchRetrieval({ ...input, sources }, session, budget, { operation: 'recall' }),
      ).toThrow();
    expect(() =>
      decodeResearchRetrieval({ ...input, items: [] }, session, budget, { operation: 'recall' }),
    ).toThrow();
    const second = { ...source, span: { ...source.span, start: 100, end: 100 + source.span.end } };
    const two = {
      receipt: {
        ...input.receipt,
        sourceBytes: input.receipt.sourceBytes * 2,
        hits: [
          ...input.receipt.hits,
          { ...input.receipt.hits[0], chunkId: 'chunk:2', spans: [second.span] },
        ],
      },
      sources: [second, source],
    };
    expect(() => decodeResearchRetrieval(two, session, budget, { operation: 'recall' })).toThrow();
  });
  it('binds episode reads to the requested ID without fabricating ranking', () => {
    const input = response();
    const expanded = {
      ...input,
      receipt: {
        ...input.receipt,
        operation: 'readEpisode',
        hits: input.receipt.hits.map((hit) => ({ ...hit, lexicalRank: null, fusionScore: 0 })),
      },
    };
    expect(
      decodeResearchRetrieval(expanded, session, budget, {
        operation: 'readEpisode',
        episodeId: 'episode:1',
      }),
    ).toEqual(expanded);
    expect(() =>
      decodeResearchRetrieval(expanded, session, budget, { operation: 'recall' }),
    ).toThrow();
    expect(() =>
      decodeResearchRetrieval(expanded, session, budget, {
        operation: 'readEpisode',
        episodeId: 'other',
      }),
    ).toThrow();
  });
  it('retains an empty successful search without inferring source completeness', () => {
    const input = response();
    const empty = {
      receipt: { ...input.receipt, hits: [], sourceBytes: 0, contextTokens: 0 },
      sources: [],
    };
    expect(decodeResearchRetrieval(empty, session, budget, { operation: 'recall' })).toEqual(empty);
    expect(() =>
      decodeResearchRetrieval(empty, { ...session, mode: 'hybrid' }, budget, {
        operation: 'recall',
      }),
    ).toThrow();
  });
});
