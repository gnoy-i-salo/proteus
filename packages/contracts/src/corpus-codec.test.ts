import invalidWire from './fixtures/invalid-corpus-wire.json';
import { describe, expect, it } from 'vitest';
import { decodeCorpusSnapshot } from './corpus-codec';
import type { CorpusSnapshot } from './corpus';

const exact = '  é 👩🏽‍🔬\r\nисточник\t';
const snapshot: CorpusSnapshot = {
  schemaVersion: 1,
  id: `snapshot:${exact}`,
  corpusId: 'corpus:1',
  parentSnapshotId: null,
  createdAt: '2026-11-07T12:00:00.123456789+00:00',
  bindingIds: ['binding:1'],
  documentRefs: [{ documentId: `document:${exact}`, revisionId: 'revision:1' }],
  episodeIds: [],
  splitVersion: 'pending:1',
  generation: 1,
  coverage: {
    loadedDocuments: 1,
    indexedDocuments: 0,
    analyzedDocuments: 0,
    usedDocuments: 0,
    totalDocuments: 1,
    loadedBytes: 400,
    complete: true,
    limitations: [exact],
  },
};

describe('native corpus snapshot boundary', () => {
  it('preserves exact identifiers, nanosecond timestamps and separate coverage counters', () => {
    expect(decodeCorpusSnapshot(snapshot)).toEqual(snapshot);
    const withParent = { ...snapshot, parentSnapshotId: 'snapshot:parent', generation: 2 };
    expect(decodeCorpusSnapshot(withParent)).toEqual(withParent);
  });

  it('accepts an honestly empty acquisition without invented episodes or processed documents', () => {
    const empty = {
      ...snapshot,
      documentRefs: [],
      coverage: { ...snapshot.coverage, loadedDocuments: 0, totalDocuments: 0, loadedBytes: 0 },
    };
    expect(decodeCorpusSnapshot(empty)).toEqual(empty);
  });

  it.each(['2000-02-29T12:00:00Z', '2024-02-29T12:00:00.1-03:30'])(
    'retains valid calendar timestamps %s',
    (createdAt) => {
      expect(decodeCorpusSnapshot({ ...snapshot, createdAt }).createdAt).toBe(createdAt);
    },
  );

  it('keeps revision identity as an exact pair and returns independent metadata', () => {
    const documentRefs = [
      { documentId: 'a:b', revisionId: 'c' },
      { documentId: 'a', revisionId: 'b:c' },
      { documentId: 'a', revisionId: 'another revision' },
    ];
    const decoded = decodeCorpusSnapshot({ ...snapshot, documentRefs });
    expect(decoded.documentRefs).toEqual(documentRefs);
    expect(decoded.documentRefs).not.toBe(documentRefs);
    expect(decoded.documentRefs[0]).not.toBe(documentRefs[0]);
    expect(decoded.coverage).not.toBe(snapshot.coverage);
    expect(decoded.coverage.limitations).not.toBe(snapshot.coverage.limitations);
    expect(decoded.bindingIds).not.toBe(snapshot.bindingIds);
  });

  it.each([
    { schemaVersion: 2 },
    { schemaVersion: undefined },
    invalidWire.schemaAlias,
    invalidWire.parentAlias,
    { id: ' ' },
    { parentSnapshotId: '' },
    { generation: 0 },
    { generation: 1.5 },
    { generation: Number.MAX_SAFE_INTEGER + 1 },
    { bindingIds: ['binding:1', 'binding:1'] },
    { episodeIds: ['episode:1', 'episode:1'] },
    { splitVersion: '' },
    { documentRefs: [invalidWire.revisionAlias] },
    { documentRefs: [{ documentId: 'doc', revisionId: '' }] },
    { documentRefs: [snapshot.documentRefs[0], snapshot.documentRefs[0]] },
    { createdAt: '2026-02-30T12:00:00Z' },
    { createdAt: '2026-02-29T12:00:00Z' },
    { createdAt: '1900-02-29T12:00:00Z' },
    { createdAt: '2026-13-01T12:00:00Z' },
    { createdAt: '2026-01-00T12:00:00Z' },
    { createdAt: '2026-11-07T24:00:00Z' },
    { createdAt: '2026-11-07T12:60:00Z' },
    { createdAt: '2026-11-07T12:00:00+24:00' },
    { createdAt: '2026-11-07T12:00:00+00:60' },
    { createdAt: '2026-11-07T12:00:00.1234567890Z' },
    { createdAt: 'yesterday' },
    { createdAt: '2026-11-07' },
    { createdAt: '2026-11-07T12:00:00' },
    { coverage: { ...snapshot.coverage, loadedDocuments: -1 } },
    { coverage: { ...snapshot.coverage, indexedDocuments: 2 } },
    { coverage: { ...snapshot.coverage, usedDocuments: 2 } },
    { coverage: { ...snapshot.coverage, totalDocuments: 2 } },
    { coverage: { ...snapshot.coverage, ...invalidWire.coverageAlias } },
  ])('rejects corrupted, conflicting or unsupported metadata: %j', (changed) => {
    expect(() => decodeCorpusSnapshot({ ...snapshot, ...changed })).toThrow();
  });
});
