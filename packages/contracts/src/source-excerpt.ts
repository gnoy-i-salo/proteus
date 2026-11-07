import { projectBoundRevision } from './corpus';
import type { SourceBinding, DocumentRevision, SourceSpan, SourceExcerpt } from './corpus';

/** Structural projection only. Native access and expiry are rechecked at each disclosure. */
export function projectBoundExcerpt(
  binding: SourceBinding,
  revision: DocumentRevision,
  span: SourceSpan,
): SourceExcerpt {
  const projected = projectBoundRevision(binding, revision);
  const bytes = new TextEncoder().encode(projected.document.text);
  const decoder = new TextDecoder('utf-8', { fatal: true, ignoreBOM: true });
  if (
    span.documentId !== projected.document.id ||
    span.revisionId !== projected.revisionId ||
    span.unit !== 'utf8Bytes' ||
    !Number.isSafeInteger(span.start) ||
    !Number.isSafeInteger(span.end) ||
    span.start < 0 ||
    span.end <= span.start ||
    span.end > bytes.length ||
    decoder.decode(bytes) !== projected.document.text
  )
    throw new Error('Source excerpt reference or UTF8 range is invalid.');
  const text = decoder.decode(bytes.subarray(span.start, span.end));
  const document = projected.document;
  return {
    span: {
      documentId: span.documentId,
      revisionId: span.revisionId,
      unit: span.unit,
      start: span.start,
      end: span.end,
    },
    text,
    document: {
      id: document.id,
      provider: document.provider,
      resourceId: document.resourceId,
      authorId: document.authorId,
      authorLabel: document.authorLabel,
      authorship: document.authorship,
      kind: document.kind,
      mode: document.mode,
      sourceUrl: document.sourceUrl,
      createdAt: document.createdAt,
      retrievedAt: document.retrievedAt,
      contentHash: document.contentHash,
      replyTo: document.replyTo,
      isForwarded: document.isForwarded,
      truncated: document.truncated,
      partition: document.partition,
    },
    role: projected.role,
    speakerRelation: projected.speakerRelation,
    canSupportOwnWords: projected.canSupportOwnWords,
    relations: projected.relations,
  };
}
