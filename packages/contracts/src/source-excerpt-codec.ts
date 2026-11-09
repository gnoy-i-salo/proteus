import type { DocumentRelation, SourceExcerpt, SourceExcerptMetadata, SourceSpan } from './corpus';
import { decodeSourceDocumentMetadata } from './source-document-codec';
import {
  choice,
  fields,
  flag,
  id,
  integer,
  list,
  nullableText,
  text,
} from './corpus-wire-validation';

const providers = ['telegram', 'vk', 'instagram', 'github', 'web', 'savedPage'] as const;
const encoder = new TextEncoder();
const decoder = new TextDecoder('utf-8', { fatal: true, ignoreBOM: true });
function span(value: unknown): SourceSpan {
  const input = fields(value, ['documentId', 'revisionId', 'unit', 'start', 'end']);
  const start = integer(input.start);
  const end = integer(input.end);
  if (input.unit !== 'utf8Bytes' || end <= start) throw new Error('Неверные границы выдержки.');
  return {
    documentId: id(input.documentId),
    revisionId: id(input.revisionId),
    unit: 'utf8Bytes',
    start,
    end,
  };
}
function reference(value: unknown): DocumentRelation['original'] {
  if (value === null) return null;
  const input = fields(value, ['documentId', 'revisionId']);
  return { documentId: id(input.documentId), revisionId: id(input.revisionId) };
}
function relation(value: unknown): DocumentRelation {
  const input = fields(value, ['kind', 'original', 'originalAuthor', 'span']);
  const author = fields(input.originalAuthor, ['provider', 'id', 'label']);
  return {
    kind: choice(input.kind, ['reply', 'quote', 'forward']),
    original: reference(input.original),
    originalAuthor: {
      provider: author.provider === null ? null : choice(author.provider, providers),
      id: nullableText(author.id),
      label: nullableText(author.label),
    },
    span: input.span === null ? null : span(input.span),
  };
}
function metadata(value: unknown): SourceExcerptMetadata {
  const input = fields(value, [
    'schemaVersion',
    'sourceFormat',
    'resourceLabel',
    'title',
    'authorLabels',
    'originalObjectId',
    'limitations',
  ]);
  if (input.schemaVersion !== 1) throw new Error('Неизвестная версия метаданных выдержки.');
  return {
    schemaVersion: 1,
    sourceFormat: id(input.sourceFormat),
    resourceLabel: nullableText(input.resourceLabel),
    title: nullableText(input.title),
    authorLabels: boundedList(input.authorLabels).map(text),
    originalObjectId: nullableText(input.originalObjectId),
    limitations: boundedList(input.limitations).map(text),
  };
}
function boundedList(value: unknown): unknown[] {
  const values = list(value);
  if (values.length > 10000) throw new Error('Слишком много метаданных выдержки.');
  return values;
}

// Structural decoding only: live access and proof of exact original bytes remain native-owned.
export function decodeSourceExcerpt(value: unknown): SourceExcerpt {
  const keys = [
    'document',
    'span',
    'text',
    'role',
    'speakerRelation',
    'canSupportOwnWords',
    'relations',
  ];
  const hasMetadata =
    value !== null && typeof value === 'object' && Object.hasOwn(value, 'sourceMetadata');
  const input = fields(value, hasMetadata ? [...keys, 'sourceMetadata'] : keys);
  const sourceText = text(input.text);
  if (sourceText.length > 64 * 1024 * 1024) throw new Error('Слишком большая выдержка.');
  const bytes = encoder.encode(sourceText);
  if (bytes.length > 64 * 1024 * 1024 || decoder.decode(bytes) !== sourceText)
    throw new Error('Неверный UTF-8 текст выдержки.');
  const sourceSpan = span(input.span);
  const document = decodeSourceDocumentMetadata(input.document, providers);
  if (
    document.id !== sourceSpan.documentId ||
    document.partition !== 'reference' ||
    bytes.length !== sourceSpan.end - sourceSpan.start
  )
    throw new Error('Выдержка не соответствует документу или границам.');
  const result: SourceExcerpt = {
    document,
    span: sourceSpan,
    text: sourceText,
    role: choice(input.role, ['own', 'context', 'about', 'reference']),
    speakerRelation: choice(input.speakerRelation, ['target', 'context', 'unknown']),
    canSupportOwnWords: flag(input.canSupportOwnWords),
    relations: boundedList(input.relations).map(relation),
    ...(hasMetadata ? { sourceMetadata: metadata(input.sourceMetadata) } : {}),
  };
  if (
    result.canSupportOwnWords &&
    (result.role !== 'own' ||
      result.speakerRelation !== 'target' ||
      document.authorship !== 'owner' ||
      !document.authorId?.trim() ||
      document.isForwarded ||
      result.relations.some((item) => item.kind === 'quote' || item.kind === 'forward'))
  )
    throw new Error('Выдержка не может подтверждать собственные слова автора.');
  return result;
}
