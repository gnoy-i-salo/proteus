import type { SourceDocument } from '@proteus-int/contracts';

// Explicit projection keeps source identity and text, without passing grants or credentials.
export function sourceModelDocument(document: SourceDocument) {
  return {
    id: document.id,
    provider: document.provider,
    resourceId: document.resourceId,
    authorId: document.authorId,
    authorLabel: document.authorLabel,
    authorship: document.authorship,
    kind: document.kind,
    mode: document.mode,
    text: document.text,
    sourceUrl: document.sourceUrl,
    createdAt: document.createdAt,
    replyTo: document.replyTo,
    isForwarded: document.isForwarded,
    truncated: document.truncated,
  };
}
