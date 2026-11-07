import { describe, expect, it } from 'vitest';
import { projectBoundRevision } from './corpus';
import type { DocumentRevision, SourceBinding } from './corpus';

const binding: SourceBinding = {
  schemaVersion: 1,
  id: 'binding-a',
  personaId: 'persona-a',
  provider: 'telegram',
  accountId: 'account-a',
  resourceId: 'private-chat-42',
  grantId: 'grant-a',
  policyVersion: 'policy-1',
  role: 'own',
  targetAuthorId: 'author-a',
  allowedContextAuthorIds: ['author-b'],
  allowUnknownContext: false,
  period: { from: '2026-01-01T00:00:00Z', to: null, includeUndated: false },
};

const revision: DocumentRevision = {
  schemaVersion: 1,
  revisionId: 'revision-1',
  sourceKey: 'telegram:private-chat-42:message-1',
  accountId: 'account-a',
  document: {
    id: 'message-1',
    provider: 'telegram',
    resourceId: 'private-chat-42',
    authorId: 'author-a',
    authorLabel: 'Анна',
    authorship: 'owner',
    kind: 'message',
    mode: 'dialogue',
    text: '  Анна → Борис\r\n«Ёлка» 👩🏽‍🔬\nhttps://example.org/Exact-ID  ',
    sourceUrl: 'https://example.org/Exact-ID',
    createdAt: '2026-05-10T12:00:00Z',
    retrievedAt: '2026-06-01T00:00:00Z',
    contentHash: 'unchanged-original-hash',
    replyTo: 'message-0',
    isForwarded: false,
    truncated: false,
    partition: 'reference',
  },
  editedAt: null,
  relations: [
    {
      kind: 'reply',
      original: { documentId: 'message-0', revisionId: 'revision-0' },
      originalAuthor: { provider: 'telegram', id: 'author-b', label: 'Борис' },
      span: null,
    },
  ],
  attachments: [],
  provenance: {
    acquisition: 'api',
    adapterVersion: 'tdlib-v1',
    sourceFormat: 'tdlib-message',
    originalObjectId: 'object-1',
    originalByteLength: 111,
    hashAlgorithm: 'sha256',
    limitations: [],
  },
  usagePolicy: {
    grantId: 'grant-a',
    version: 'policy-1',
    allowModelProcessing: true,
    allowExport: false,
  },
};

const withDocument = (changes: Partial<DocumentRevision['document']>): DocumentRevision => ({
  ...revision,
  document: { ...revision.document, ...changes },
});

describe('structural source binding projection', () => {
  it('preserves exact identity, text, links and revision without exposing permissions', () => {
    const projected = projectBoundRevision(binding, revision);
    expect(projected.document).toEqual(revision.document);
    expect(projected.revisionId).toBe('revision-1');
    expect(projected.relations).toEqual(revision.relations);
    expect(projected.speakerRelation).toBe('target');
    expect(projected.canSupportOwnWords).toBe(true);
    expect(Object.keys(projected).sort()).toEqual([
      'canSupportOwnWords',
      'document',
      'relations',
      'revisionId',
      'role',
      'speakerRelation',
    ]);
    for (const forbidden of ['accountId', 'grantId', 'policyVersion', 'usagePolicy', 'personaId'])
      expect(projected).not.toHaveProperty(forbidden);
  });

  it('retains an explicitly permitted interlocutor without attributing their words to the target', () => {
    const other = withDocument({
      authorId: 'author-b',
      authorLabel: 'Борис',
      authorship: 'other',
    });
    const projected = projectBoundRevision(binding, other);
    expect(projected.document).toEqual(other.document);
    expect(projected.speakerRelation).toBe('context');
    expect(projected.canSupportOwnWords).toBe(false);
  });

  it('does not merge people with identical labels', () => {
    const other = withDocument({
      authorId: 'author-b',
      authorLabel: 'Анна',
      authorship: 'other',
    });
    expect(projectBoundRevision(binding, other).speakerRelation).toBe('context');
    expect(() =>
      projectBoundRevision(binding, withDocument({ authorId: 'author-c', authorLabel: 'Анна' })),
    ).toThrow();
  });

  it('does not let legacy binding-relative authorship relabel a permitted interlocutor', () => {
    const source = withDocument({ authorId: 'author-b' });
    const projected = projectBoundRevision(binding, source);
    expect(projected.document).toEqual(source.document);
    expect(projected.speakerRelation).toBe('context');
    expect(projected.canSupportOwnWords).toBe(false);
  });

  it.each(['unknown', 'other'] as const)(
    'never upgrades source authorship %s when the target ID matches',
    (authorship) => {
      const source = withDocument({ authorship });
      const projected = projectBoundRevision(binding, source);
      expect(projected.speakerRelation).toBe('target');
      expect(projected.document).toEqual(source.document);
      expect(projected.canSupportOwnWords).toBe(false);
    },
  );

  it('keeps unknown authors unknown only when their context was explicitly selected', () => {
    const unknown = withDocument({
      authorId: null,
      authorLabel: 'Анна',
      authorship: 'unknown',
    });
    expect(() => projectBoundRevision(binding, unknown)).toThrow();
    const selected = projectBoundRevision({ ...binding, allowUnknownContext: true }, unknown);
    expect(selected.speakerRelation).toBe('unknown');
    expect(selected.document.authorId).toBeNull();
    expect(selected.document.authorship).toBe('unknown');
  });

  it.each([
    ['foreign provider', withDocument({ provider: 'vk' })],
    ['foreign resource', withDocument({ resourceId: 'private-chat-99' })],
    ['foreign account', { ...revision, accountId: 'account-b' }],
    [
      'foreign grant',
      {
        ...revision,
        usagePolicy: { ...revision.usagePolicy, grantId: 'grant-b' },
      },
    ],
    [
      'stale policy',
      {
        ...revision,
        usagePolicy: { ...revision.usagePolicy, version: 'policy-0' },
      },
    ],
    [
      'model processing denied',
      {
        ...revision,
        usagePolicy: { ...revision.usagePolicy, allowModelProcessing: false },
      },
    ],
    ['holdout', withDocument({ partition: 'holdout' })],
    ['before selected period', withDocument({ createdAt: '2025-12-31T23:59:59Z' })],
    ['invalid source time', withDocument({ createdAt: 'not a time' })],
    ['undated without permission', withDocument({ createdAt: null })],
  ])('rejects %s before projection', (_name, document) => {
    expect(() => projectBoundRevision(binding, document)).toThrow();
  });

  it('checks both period bounds using instants without rewriting source timestamps', () => {
    const selected = {
      ...binding,
      period: { ...binding.period, to: '2026-05-10T12:00:00Z' },
    };
    const sameInstant = withDocument({
      createdAt: '2026-05-10T15:00:00+03:00',
    });
    expect(projectBoundRevision(selected, sameInstant).document.createdAt).toBe(
      sameInstant.document.createdAt,
    );
    expect(() =>
      projectBoundRevision(selected, withDocument({ createdAt: '2026-05-10T12:00:01Z' })),
    ).toThrow();
  });

  it('accepts explicitly permitted undated context without inventing a date', () => {
    const selected = {
      ...binding,
      period: { ...binding.period, includeUndated: true },
    };
    expect(
      projectBoundRevision(selected, withDocument({ createdAt: null })).document.createdAt,
    ).toBeNull();
  });

  it('preserves quoted and forwarded authorship and cannot treat a forward as target-authored', () => {
    const forwarded: DocumentRevision = {
      ...withDocument({ isForwarded: true }),
      relations: [
        {
          kind: 'forward',
          original: null,
          originalAuthor: { provider: 'telegram', id: 'author-b', label: 'Борис' },
          span: null,
        },
      ],
    };
    const result = projectBoundRevision(binding, forwarded);
    expect(result.document.authorId).toBe('author-a');
    expect(result.document).toEqual(forwarded.document);
    expect(result.canSupportOwnWords).toBe(false);
    expect(result.relations).toEqual(forwarded.relations);
    expect(result.document.text).toBe(forwarded.document.text);
  });

  it('retains exact quoted spans and the original author without claiming the quote as own text', () => {
    const quoted: DocumentRevision = {
      ...revision,
      relations: [
        {
          kind: 'quote',
          original: null,
          originalAuthor: { provider: 'vk', id: 'author-a', label: 'Анна' },
          span: {
            documentId: 'message-1',
            revisionId: 'revision-1',
            unit: 'utf8Bytes',
            start: 0,
            end: 10,
          },
        },
      ],
    };
    const result = projectBoundRevision(binding, quoted);
    expect(result.document).toEqual(quoted.document);
    expect(result.canSupportOwnWords).toBe(false);
    expect(result.relations).toEqual(quoted.relations);
    expect(result.document.text).toBe(revision.document.text);
  });

  it('projects only known source fields even if a runtime document carries unrelated metadata', () => {
    const enriched = {
      ...revision,
      document: { ...revision.document, grantId: 'private-grant', token: 'synthetic-secret' },
    };
    expect(projectBoundRevision(binding, enriched).document).toEqual(revision.document);
  });

  it.each([
    { from: 'invalid', to: null, includeUndated: false },
    { from: '2026-05-11T00:00:00Z', to: '2026-05-10T00:00:00Z', includeUndated: true },
  ])('rejects invalid time scope rather than widening it', (period) => {
    expect(() => projectBoundRevision({ ...binding, period }, revision)).toThrow();
  });

  it('does not promote reference material to the selected author’s own words', () => {
    const result = projectBoundRevision({ ...binding, role: 'reference' }, revision);
    expect(result.role).toBe('reference');
    expect(result.document).toEqual(revision.document);
    expect(result.canSupportOwnWords).toBe(false);
  });

  it('accepts selected web/saved-page capability with the same explicit author scope', () => {
    for (const provider of ['web', 'savedPage'] as const) {
      const selected = {
        ...binding,
        provider,
        role: 'reference' as const,
        targetAuthorId: null,
        allowedContextAuthorIds: ['author-a'],
      };
      const document = withDocument({ provider });
      expect(projectBoundRevision(selected, document).speakerRelation).toBe('context');
    }
  });
});

describe('revision provenance limits', () => {
  const legacy = {
    acquisition: 'archive',
    adapterVersion: 'legacy-source-record-v1',
    sourceFormat: 'proteusEncryptedSourceRecord',
    originalObjectId: null,
    originalByteLength: null,
    hashAlgorithm: 'sha256',
    limitations: [
      'originalObjectIdUnavailable',
      'originalBytesUnavailable',
      'attachmentsUnavailable',
      'relationsIncomplete',
    ],
  };
  const withProvenance = (provenance: unknown): DocumentRevision => ({
    ...revision,
    provenance: provenance as DocumentRevision['provenance'],
  });
  it.each([
    { ...revision.provenance, originalObjectId: null },
    { ...revision.provenance, limitations: ['originalObjectIdUnavailable'] },
    { ...revision.provenance, limitations: ['originalBytesUnavailable'] },
    { ...revision.provenance, originalByteLength: null },
    { ...revision.provenance, originalObjectId: '' },
    { ...revision.provenance, originalByteLength: -1 },
    { ...revision.provenance, originalByteLength: 1.5 },
    { ...revision.provenance, originalByteLength: Number.MAX_SAFE_INTEGER + 1 },
    { ...revision.provenance, limitations: undefined },
    { ...revision.provenance, limitations: ['invented'] },
    { ...revision.provenance, limitations: ['relationsIncomplete', 'relationsIncomplete'] },
    { ...legacy, limitations: ['originalObjectIdUnavailable', 'originalBytesUnavailable'] },
    { ...legacy, originalObjectId: revision.document.id },
    { ...legacy, originalByteLength: new TextEncoder().encode(revision.document.text).length },
    { ...legacy, adapterVersion: 'legacy-source-record-v99' },
    { ...legacy, sourceFormat: 'newlyClaimedOriginal' },
  ])('rejects missing, contradictory or fabricated provenance (%#)', (provenance) => {
    expect(() => projectBoundRevision(binding, withProvenance(provenance))).toThrow();
  });
  it('preserves an explicit legacy observation without claiming missing attachment or relation completeness', () => {
    const stored = withProvenance(legacy);
    const result = projectBoundRevision(binding, stored);
    expect(result.document).toEqual(revision.document);
    expect(result.relations).toEqual(revision.relations);
    expect(stored.provenance).toEqual(legacy);
    expect(result).not.toHaveProperty('provenance');
  });
  it('allows a newly acquired source with trustworthy empty attachment and relation lists', () => {
    const acquired = { ...revision, attachments: [], relations: [] };
    expect(projectBoundRevision(binding, acquired).document).toEqual(revision.document);
  });
  it('allows explicit unavailable native metadata without upgrading or rewriting source identity', () => {
    const provenance = {
      ...revision.provenance,
      originalObjectId: null,
      originalByteLength: null,
      limitations: ['originalObjectIdUnavailable', 'originalBytesUnavailable'],
    };
    expect(projectBoundRevision(binding, withProvenance(provenance)).document).toEqual(
      revision.document,
    );
  });
});
