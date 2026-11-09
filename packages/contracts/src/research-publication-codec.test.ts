import { describe, expect, it } from 'vitest';
import { decodeResearchPublication } from './research-publication-codec';
import type { ResearchPublicationDraft, ResearchPublicationReceipt } from './memory';
import type { ResearchSessionDescriptor } from './research-memory';

const session: ResearchSessionDescriptor = {
  schemaVersion: 1,
  id: 'session:1',
  personaId: 'persona:1',
  corpusSnapshotId: 'corpus:1',
  policyVersion: 'policy:1',
  mode: 'lexical',
  indexGeneration: 1,
  expiresAt: '2026-11-09T03:01:00.000Z',
};
const span = {
  documentId: 'document:1',
  revisionId: 'revision:1',
  unit: 'utf8Bytes' as const,
  start: 0,
  end: 10,
};
const draft: ResearchPublicationDraft = {
  schemaVersion: 1,
  extractionVersion: 'extract:1',
  items: [
    {
      draftId: 'draft:1',
      kind: 'episode',
      text: '  Анна\r\n',
      sourceSpans: [span],
      episodeIds: ['episode:1'],
      eventAt: null,
      support: 'observed',
      contradictions: [],
      limitations: [],
    },
  ],
  analyzedRefs: [{ documentId: span.documentId, revisionId: span.revisionId }],
  contextLeaseIds: ['lease:1'],
};
function response(): ResearchPublicationReceipt {
  const publishedAt = '2026-11-09T03:00:00.000Z';
  return {
    schemaVersion: 1,
    id: 'publication:1',
    snapshot: {
      schemaVersion: 1,
      id: 'memory:1',
      corpusSnapshotId: session.corpusSnapshotId,
      itemIds: ['item:1'],
      extractionVersion: draft.extractionVersion,
      createdAt: publishedAt,
    },
    itemIds: [{ draftId: 'draft:1', itemId: 'item:1' }],
    coverage: {
      loadedDocuments: 10,
      indexedDocuments: 4,
      analyzedDocuments: 1,
      usedDocuments: 1,
      totalDocuments: 10,
      loadedBytes: 1000,
      complete: true,
      limitations: ['  Только выбранные источники\r\n'],
    },
    analyzedSpans: [span],
    publishedAt,
  };
}
function decode(value: unknown) {
  return decodeResearchPublication(value, session, draft);
}
describe('native research publication response', () => {
  it('retains native IDs and acquisition coverage without inferring complete analysis', () => {
    const value = response();
    const original = JSON.stringify({ value, draft });
    expect(decode(value)).toEqual(value);
    expect(JSON.stringify({ value, draft })).toBe(original);
  });
  it.each([
    { schemaVersion: 2 },
    { id: '' },
    { unknown: true },
    { publishedAt: '2026-11-09T03:00:01.000Z' },
    { itemIds: [] },
    { itemIds: [{ draftId: 'foreign', itemId: 'item:1' }] },
    { itemIds: [{ draftId: 'draft:1', itemId: 'foreign' }] },
    { analyzedSpans: [] },
    { analyzedSpans: [{ ...span, revisionId: 'foreign' }] },
    { analyzedSpans: [span, span] },
    { analyzedSpans: [{ ...span, end: 0 }] },
    { analyzedSpans: [{ ...span, unit: 'characters' }] },
  ])('rejects inconsistent publication metadata: %j', (patch) => {
    expect(() => decode({ ...response(), ...patch })).toThrow();
  });
  it.each([
    { schemaVersion: 2 },
    { corpusSnapshotId: 'foreign' },
    { extractionVersion: 'foreign' },
    { itemIds: ['foreign'] },
    { itemIds: ['item:1', 'item:1'] },
    { id: '\n' },
    { createdAt: '2026-11-09T03:00:01.000Z' },
    { unknown: true },
  ])('rejects an unrelated memory snapshot: %j', (patch) => {
    const value = response();
    expect(() => decode({ ...value, snapshot: { ...value.snapshot, ...patch } })).toThrow();
  });
  it.each([
    { loadedDocuments: 3 },
    { indexedDocuments: 0 },
    { analyzedDocuments: 2 },
    { usedDocuments: 0 },
    { totalDocuments: 11 },
    { loadedBytes: -1 },
    { unknown: true },
    { limitations: [''] },
    { limitations: Array<string>(17).fill('limit') },
  ])('rejects contradictory or unbounded coverage: %j', (patch) => {
    const value = response();
    expect(() => decode({ ...value, coverage: { ...value.coverage, ...patch } })).toThrow();
  });
  it('requires native IDs to match every draft in order and remain unique', () => {
    const value = response();
    const twoDrafts = {
      ...draft,
      items: [...draft.items, { ...draft.items[0]!, draftId: 'draft:2' }],
    };
    const two = {
      ...value,
      snapshot: { ...value.snapshot, itemIds: ['item:1', 'item:2'] },
      itemIds: [
        { draftId: 'draft:1', itemId: 'item:1' },
        { draftId: 'draft:2', itemId: 'item:2' },
      ],
    };
    expect(decodeResearchPublication(two, session, twoDrafts)).toEqual(two);
    expect(() =>
      decodeResearchPublication(
        { ...two, itemIds: [...two.itemIds].reverse() },
        session,
        twoDrafts,
      ),
    ).toThrow();
    expect(() =>
      decodeResearchPublication(
        {
          ...two,
          itemIds: two.itemIds.map((item) => ({ ...item, itemId: 'item:1' })),
          snapshot: { ...two.snapshot, itemIds: ['item:1', 'item:1'] },
        },
        session,
        twoDrafts,
      ),
    ).toThrow();
  });
  it('does not merge adjacent analyzed spans to fabricate support for an item', () => {
    expect(() =>
      decode({
        ...response(),
        analyzedSpans: [
          { ...span, end: 5 },
          { ...span, start: 5 },
        ],
      }),
    ).toThrow();
    expect(
      decode({ ...response(), analyzedSpans: [{ ...span, end: 20 }] }).analyzedSpans[0]?.end,
    ).toBe(20);
  });
  it('requires a real canonical UTC native publication time', () => {
    for (const publishedAt of ['2026-02-30T03:00:00.000Z', '2026-11-09T03:00:00Z', 'invalid']) {
      const value = response();
      expect(() =>
        decode({ ...value, publishedAt, snapshot: { ...value.snapshot, createdAt: publishedAt } }),
      ).toThrow();
    }
  });
  it('retains a valid empty publication and unknown acquisition total', () => {
    const value = response();
    const empty = {
      ...value,
      snapshot: { ...value.snapshot, itemIds: [] },
      itemIds: [],
      analyzedSpans: [],
      coverage: {
        ...value.coverage,
        analyzedDocuments: 0,
        usedDocuments: 0,
        complete: false,
        totalDocuments: null,
      },
    };
    expect(
      decodeResearchPublication(empty, session, {
        ...draft,
        items: [],
        analyzedRefs: [],
        contextLeaseIds: [],
      }),
    ).toEqual(empty);
  });
});
