import { describe, expect, it } from 'vitest';
import {
  decodeLegacySourceAction,
  decodeLegacySourceCoverage,
  decodeLegacySourceGrant,
  decodeLegacySourceGrantRequest,
  decodeLegacySourceResult,
  encodeLegacySourceAction,
  encodeLegacySourceGrantRequest,
} from './source-wire-codec';

const request = {
  provider: 'telegram',
  'resource-id': 'source-id: exact',
  'author-id': null,
  from: '2026-01-01',
  to: null,
  'allow-model-processing': false,
  operations: ['describe', 'read'],
  budget: {
    'max-steps': 4,
    'max-documents': 100,
    'max-bytes': 4096,
    'max-duration-ms': 3000,
  },
};
const coverage = {
  documents: 2,
  'total-documents': null,
  bytes: 123,
  complete: false,
  truncated: true,
  'not-analyzed': 1,
  unavailable: ['точный исходный текст\r\n'],
};
const document = {
  id: 'message-id',
  provider: 'telegram',
  'resource-id': 'source-id: exact',
  'author-id': null,
  authorship: 'other',
  'author-label': 'Мария — сестра Ивана',
  kind: 'message',
  mode: 'dialogue',
  text: '  Иван: **да**\r\n  👩🏽‍🔬\t source-url остаётся текстом ',
  'source-url': 'https://example.test/source-url?author-id=unchanged',
  'created-at': null,
  'retrieved-at': '2026-11-07T00:00:00Z',
  'content-hash': 'opaque-hash',
  'reply-to': 'message-previous',
  'is-forwarded': true,
  truncated: false,
  partition: 'holdout',
};

describe('source native wire migration', () => {
  it('converts request fields explicitly without granting model processing or normalizing values', () => {
    const decoded = decodeLegacySourceGrantRequest(request);
    expect(decoded).toEqual({
      provider: 'telegram',
      resourceId: request['resource-id'],
      authorId: null,
      from: request.from,
      to: null,
      allowModelProcessing: false,
      operations: request.operations,
      budget: {
        maxSteps: 4,
        maxDocuments: 100,
        maxBytes: 4096,
        maxDurationMs: 3000,
      },
    });
    expect(encodeLegacySourceGrantRequest(decoded)).toEqual(request);
    expect(request['allow-model-processing']).toBe(false);
  });
  it('preserves absent optional request values separately from explicit null', () => {
    const { 'author-id': authorId, from, to, ...absent } = request;
    expect(authorId).toBeNull();
    expect(from).toBeTruthy();
    expect(to).toBeNull();
    const decoded = decodeLegacySourceGrantRequest(absent);
    expect(decoded).not.toHaveProperty('authorId');
    expect(decoded).not.toHaveProperty('from');
    expect(encodeLegacySourceGrantRequest(decoded)).toEqual(absent);
  });
  it('preserves flattened grant identity and never interprets its presence as authorization', () => {
    expect(
      decodeLegacySourceGrant({
        ...request,
        id: 'grant-id',
        'account-id': 'account-id',
        'snapshot-id': 'snapshot-id',
        'expires-at': '2020-01-01T00:00:00Z',
      }),
    ).toEqual({
      ...decodeLegacySourceGrantRequest(request),
      id: 'grant-id',
      accountId: 'account-id',
      snapshotId: 'snapshot-id',
      expiresAt: '2020-01-01T00:00:00Z',
    });
  });
  it.each([
    { action: 'describe' },
    { action: 'coverage' },
    { action: 'search', query: ' exact\r\n author-id ', limit: 7 },
    { action: 'collect', cursor: null, limit: 2 },
    { action: 'collect', limit: 2 },
    { action: 'read', 'document-ids': ['source-url', 'имя 👩🏽‍🔬'] },
  ])('roundtrips only the selected action schema %j', (action) => {
    const decoded = decodeLegacySourceAction(action);
    expect(encodeLegacySourceAction(decoded)).toEqual(action);
    if (action.action === 'read')
      expect(decoded).toHaveProperty('documentIds', action['document-ids']);
  });
  it('preserves coverage unknown totals and exact documents via the contracts decoder', () => {
    const result = decodeLegacySourceResult({
      'snapshot-id': 'snapshot-id',
      documents: [document],
      coverage,
      cursor: null,
      limitations: ['факт?\r\n гипотеза'],
    });
    expect(result.snapshotId).toBe('snapshot-id');
    expect(result.documents[0]).toMatchObject({
      authorId: null,
      authorLabel: document['author-label'],
      text: document.text,
      replyTo: document['reply-to'],
      sourceUrl: document['source-url'],
      contentHash: document['content-hash'],
      isForwarded: true,
      partition: 'holdout',
    });
    expect(result.coverage).toEqual({
      documents: 2,
      totalDocuments: null,
      bytes: 123,
      complete: false,
      truncated: true,
      notAnalyzed: 1,
      unavailable: coverage.unavailable,
    });
    expect(result.cursor).toBeNull();
    expect(result.limitations).toEqual(['факт?\r\n гипотеза']);
  });
  it.each([
    { ...request, resourceId: request['resource-id'] },
    { ...request, extra: 'no' },
    { ...request, provider: 'invented' },
    { ...request, operations: ['shell'] },
    { ...request, budget: { ...request.budget, maxSteps: 4 } },
    { ...request, budget: { ...request.budget, 'max-steps': 1.1 } },
    {
      ...request,
      budget: { ...request.budget, 'max-bytes': Number.MAX_SAFE_INTEGER + 1 },
    },
    { ...request, 'allow-model-processing': 'true' },
    { ...request, 'author-id': undefined },
  ])('rejects mixed aliases, unknown fields and invalid request shapes %j', (value) => {
    expect(() => decodeLegacySourceGrantRequest(value)).toThrow();
  });
  it.each([
    { action: 'read', 'document-ids': [], documentIds: [] },
    { action: 'read', 'document-ids': [42] },
    { action: 'describe', query: 'not allowed' },
    { action: 'search', query: 'x', limit: -1 },
    { action: 'collect', cursor: false, limit: 2 },
    { action: 'finish', reason: 'not a native action' },
  ])('rejects action collisions and unsupported alternatives %j', (value) => {
    expect(() => decodeLegacySourceAction(value)).toThrow();
  });
  it('rejects invalid coverage and nested aliases instead of partially converting results', () => {
    for (const value of [
      { ...coverage, totalDocuments: null },
      { ...coverage, bytes: NaN },
      { ...coverage, 'not-analyzed': -1 },
      { ...coverage, 'total-documents': undefined },
    ])
      expect(() => decodeLegacySourceCoverage(value)).toThrow();
    expect(() =>
      decodeLegacySourceResult({
        'snapshot-id': 'id',
        documents: [{ ...document, authorId: null }],
        coverage,
        cursor: null,
        limitations: [],
      }),
    ).toThrow();
  });
  it('validates encoder input too and rejects excessive source payload before conversion', () => {
    const canonical = decodeLegacySourceGrantRequest(request);
    expect(() =>
      encodeLegacySourceGrantRequest({ ...canonical, 'resource-id': 'mixed' }),
    ).toThrow();
    expect(() =>
      encodeLegacySourceAction({
        action: 'read',
        documentIds: [],
        'document-ids': [],
      }),
    ).toThrow();
    expect(() =>
      decodeLegacySourceResult({
        'snapshot-id': 'id',
        documents: [{ ...document, text: 'x'.repeat(32 * 1024 * 1024) }],
        coverage,
        cursor: null,
        limitations: [],
      }),
    ).toThrow();
  });
});

it('requires nullable author and period fields on issued grants, independently of request omission', () => {
  const wireGrant = {
    ...request,
    id: 'issued-grant',
    'account-id': 'account-id',
    'snapshot-id': 'snapshot-id',
    'expires-at': '2026-11-07T12:00:00Z',
  };
  const canonical: { authorId: string | null; from: string | null; to: string | null } =
    decodeLegacySourceGrant(wireGrant);
  expect(canonical.authorId).toBeNull();
  expect(canonical.from).toBe(request.from);
  expect(canonical.to).toBeNull();
  for (const key of ['author-id', 'from', 'to']) {
    const omitted = Object.fromEntries(
      Object.entries(wireGrant).filter(([field]) => field !== key),
    );
    expect(() => decodeLegacySourceGrant(omitted)).toThrow();
  }
  const { 'author-id': authorId, from, to, ...optionalRequest } = request;
  expect(authorId).toBeNull();
  expect(from).toBeTruthy();
  expect(to).toBeNull();
  expect(decodeLegacySourceGrantRequest(optionalRequest)).not.toHaveProperty('authorId');
});
