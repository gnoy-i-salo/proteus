import type { SourceExcerpt, SourceSpan } from '@proteus-int/contracts';
import type { MemoryAnalysisScope } from '../parse-memory-analysis';

export const encoder = new TextEncoder();

export interface ExcerptFixtureOptions {
  readonly text?: string;
  readonly documentId?: string;
  readonly revisionId?: string;
  readonly start?: number;
  readonly role?: SourceExcerpt['role'];
  readonly speakerRelation?: SourceExcerpt['speakerRelation'];
  readonly canSupportOwnWords?: boolean;
  readonly authorId?: string | null;
  readonly authorLabel?: string | null;
  readonly authorship?: SourceExcerpt['document']['authorship'];
  readonly isForwarded?: boolean;
  readonly relations?: SourceExcerpt['relations'];
  readonly partition?: SourceExcerpt['document']['partition'];
}

export function memoryExcerpt(options: ExcerptFixtureOptions = {}): SourceExcerpt {
  const text = options.text ?? '  Анна → Борис\r\n«Ёлка» 👩🏽‍🔬  ';
  const documentId = options.documentId ?? 'message-1';
  const revisionId = options.revisionId ?? 'revision-1';
  const start = options.start ?? 0;
  return {
    span: {
      documentId,
      revisionId,
      unit: 'utf8Bytes',
      start,
      end: start + encoder.encode(text).length,
    },
    text,
    document: {
      id: documentId,
      provider: 'telegram',
      resourceId: 'private-chat-42',
      authorId: options.authorId === undefined ? 'author-ann' : options.authorId,
      authorLabel: options.authorLabel === undefined ? 'Анна' : options.authorLabel,
      authorship: options.authorship ?? 'owner',
      kind: 'message',
      mode: 'dialogue',
      sourceUrl: 'https://example.org/ann',
      createdAt: '2026-05-10T12:00:00Z',
      retrievedAt: '2026-06-01T00:00:00Z',
      contentHash: `hash-${documentId}`,
      replyTo: null,
      isForwarded: options.isForwarded ?? false,
      truncated: false,
      partition: options.partition ?? 'reference',
    },
    role: options.role ?? 'own',
    speakerRelation: options.speakerRelation ?? 'target',
    canSupportOwnWords: options.canSupportOwnWords ?? true,
    relations: options.relations ?? [],
  };
}

export function fullSpan(excerpt: SourceExcerpt): SourceSpan {
  return { ...excerpt.span };
}

export function memoryScope(
  sources: readonly { episodeId: string; excerpt: SourceExcerpt }[],
  options: { existingItemIds?: readonly string[]; maxItems?: number } = {},
): MemoryAnalysisScope {
  return {
    sources,
    existingItemIds: options.existingItemIds ?? [],
    maxItems: options.maxItems ?? 100,
  };
}
