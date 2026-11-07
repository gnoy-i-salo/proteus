import type {
  PersonInput,
  SourceDocument,
  SourceGrant,
  SourceResult,
} from '@proteus-int/contracts';

export const sourceInput: PersonInput = { label: 'Ada Lovelace', context: '', publicUrls: [] };
export const sourceGrant: SourceGrant = {
  id: 'grant-1',
  provider: 'telegram',
  resourceId: 'private-chat-123',
  accountId: 'private-account',
  snapshotId: 'snapshot-1',
  authorId: 'author-ada',
  from: null,
  to: null,
  expiresAt: '2099-01-01T00:00:00Z',
  allowModelProcessing: true,
  operations: ['describe', 'search', 'collect', 'read', 'coverage'],
  budget: { maxSteps: 12, maxDocuments: 100, maxBytes: 100000, maxDurationMs: 10000 },
};
export function sourceDocument(id = 'd1', text = '  привет))\n\nда?? 🙂  '): SourceDocument {
  return {
    id,
    provider: 'telegram',
    resourceId: sourceGrant.resourceId,
    authorId: 'author-ada',
    authorship: 'owner',
    authorLabel: 'Ada Lovelace',
    kind: 'message',
    mode: 'dialogue',
    text,
    sourceUrl: 'https://example.org/ada',
    createdAt: '2026-01-01T00:00:00Z',
    retrievedAt: '2026-02-01T00:00:00Z',
    contentHash: `hash-${id}`,
    replyTo: null,
    isForwarded: false,
    truncated: false,
    partition: 'reference',
  };
}
export function sourceResult(
  documents: SourceDocument[],
  cursor: string | null = null,
): SourceResult {
  return {
    snapshotId: 'snapshot-1',
    documents,
    cursor,
    limitations: [],
    coverage: {
      documents: documents.length,
      totalDocuments: cursor ? null : documents.length,
      bytes: documents.reduce((n, doc) => n + new TextEncoder().encode(doc.text).length, 0),
      complete: cursor === null,
      truncated: false,
      notAnalyzed: 0,
      unavailable: [],
    },
  };
}
