import type { SourceDocument } from '@proteus-int/contracts';

// Explicit projection keeps source identity and text, without passing grants or credentials.
export function sourceModelDocument(document: SourceDocument) {
  return {
    id: document.id,
    provider: document.provider,
    'resource-id': document['resource-id'],
    'author-id': document['author-id'],
    'author-label': document['author-label'],
    authorship: document.authorship,
    kind: document.kind,
    mode: document.mode,
    text: document.text,
    'source-url': document['source-url'],
    'created-at': document['created-at'],
    'reply-to': document['reply-to'],
    'is-forwarded': document['is-forwarded'],
    truncated: document.truncated,
  };
}
