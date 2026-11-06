import type {
  PersonInput,
  SourceDocument,
  SourceGrant,
  SourceResult,
} from '@proteus-int/contracts';

export const sourceInput: PersonInput = { label: 'Ada Lovelace', context: '', 'public-urls': [] };
export const sourceGrant: SourceGrant = {
  id: 'grant-1',
  provider: 'telegram',
  'resource-id': 'private-chat-123',
  'account-id': 'private-account',
  'snapshot-id': 'snapshot-1',
  'author-id': 'author-ada',
  from: null,
  to: null,
  'expires-at': '2099-01-01T00:00:00Z',
  'allow-model-processing': true,
  operations: ['describe', 'search', 'collect', 'read', 'coverage'],
  budget: { 'max-steps': 12, 'max-documents': 100, 'max-bytes': 100000, 'max-duration-ms': 10000 },
};
export function sourceDocument(id = 'd1', text = '  привет))\n\nда?? 🙂  '): SourceDocument {
  return {
    id,
    provider: 'telegram',
    'resource-id': sourceGrant['resource-id'],
    'author-id': 'author-ada',
    authorship: 'owner',
    'author-label': 'Ada Lovelace',
    kind: 'message',
    mode: 'dialogue',
    text,
    'source-url': 'https://example.org/ada',
    'created-at': '2026-01-01T00:00:00Z',
    'retrieved-at': '2026-02-01T00:00:00Z',
    'content-hash': `hash-${id}`,
    'reply-to': null,
    'is-forwarded': false,
    truncated: false,
    partition: 'reference',
  };
}
export function sourceResult(
  documents: SourceDocument[],
  cursor: string | null = null,
): SourceResult {
  return {
    'snapshot-id': 'snapshot-1',
    documents,
    cursor,
    limitations: [],
    coverage: {
      documents: documents.length,
      'total-documents': cursor ? null : documents.length,
      bytes: documents.reduce((n, doc) => n + new TextEncoder().encode(doc.text).length, 0),
      complete: cursor === null,
      truncated: false,
      'not-analyzed': 0,
      unavailable: [],
    },
  };
}
