import type { SourceDocument } from './sources';

export type StoredSourceDocument = SourceDocument;

function fields(value: unknown, allowed: readonly string[]): Record<string, unknown> {
  if (!value || typeof value !== 'object' || Array.isArray(value))
    throw new Error('Неверный документ источника.');
  if (Object.keys(value).some((key) => !allowed.includes(key)))
    throw new Error('Неизвестные или смешанные поля документа.');
  return value as Record<string, unknown>;
}

function text(value: unknown): string {
  if (typeof value !== 'string') throw new Error('Неверное текстовое поле документа.');
  return value;
}

function nullableText(value: unknown): string | null {
  return value === null ? null : text(value);
}

function boolean(value: unknown): boolean {
  if (typeof value !== 'boolean') throw new Error('Неверный флаг документа.');
  return value;
}

function choice<T extends string>(value: unknown, choices: readonly T[]): T {
  const found = choices.find((item) => item === value);
  if (!found) throw new Error('Неизвестное значение поля документа.');
  return found;
}

const metadataFields = [
  'id',
  'provider',
  'resourceId',
  'authorId',
  'authorship',
  'authorLabel',
  'kind',
  'mode',
  'sourceUrl',
  'createdAt',
  'retrievedAt',
  'contentHash',
  'replyTo',
  'isForwarded',
  'truncated',
  'partition',
] as const;

// Shared metadata boundary; an excerpt has no full-document text to invent.
export function decodeSourceDocumentMetadata<Provider extends string>(
  value: unknown,
  providers: readonly Provider[],
): Omit<SourceDocument, 'text' | 'provider'> & { readonly provider: Provider } {
  const source = fields(value, metadataFields);
  return {
    id: text(source['id']),
    provider: choice(source['provider'], providers),
    resourceId: text(source['resourceId']),
    authorId: nullableText(source['authorId']),
    authorship: choice(source['authorship'], ['owner', 'other', 'unknown'] as const),
    authorLabel: nullableText(source['authorLabel']),
    kind: choice(source['kind'], ['message', 'post', 'comment', 'code', 'document'] as const),
    mode: choice(source['mode'], ['dialogue', 'comment', 'formal'] as const),
    sourceUrl: nullableText(source['sourceUrl']),
    createdAt: nullableText(source['createdAt']),
    retrievedAt: nullableText(source['retrievedAt']),
    contentHash: text(source['contentHash']),
    replyTo: nullableText(source['replyTo']),
    isForwarded: boolean(source['isForwarded']),
    truncated: boolean(source['truncated']),
    partition: choice(source['partition'], ['reference', 'holdout'] as const),
  };
}

export function decodeSourceDocument(value: unknown): SourceDocument {
  const source = fields(value, [...metadataFields, 'text']);
  const { text: sourceText, ...metadata } = source;
  const document = decodeSourceDocumentMetadata(metadata, [
    'telegram',
    'vk',
    'instagram',
    'github',
  ] as const);
  // Keep canonical property order as well as exact source strings for existing writers.
  return {
    id: document.id,
    provider: document.provider,
    resourceId: document.resourceId,
    authorId: document.authorId,
    authorship: document.authorship,
    authorLabel: document.authorLabel,
    kind: document.kind,
    mode: document.mode,
    text: text(sourceText),
    sourceUrl: document.sourceUrl,
    createdAt: document.createdAt,
    retrievedAt: document.retrievedAt,
    contentHash: document.contentHash,
    replyTo: document.replyTo,
    isForwarded: document.isForwarded,
    truncated: document.truncated,
    partition: document.partition,
  };
}

// Exact old schema only. This conversion grants no access and does not verify source claims.
export function decodeLegacySourceDocument(value: unknown): StoredSourceDocument {
  const source = fields(value, [
    'id',
    'provider',
    'resource-id',
    'author-id',
    'authorship',
    'author-label',
    'kind',
    'mode',
    'text',
    'source-url',
    'created-at',
    'retrieved-at',
    'content-hash',
    'reply-to',
    'is-forwarded',
    'truncated',
    'partition',
  ]);
  return decodeSourceDocument({
    id: source['id'],
    provider: source['provider'],
    resourceId: source['resource-id'],
    authorId: source['author-id'],
    authorship: source['authorship'],
    authorLabel: source['author-label'],
    kind: source['kind'],
    mode: source['mode'],
    text: source['text'],
    sourceUrl: source['source-url'],
    createdAt: source['created-at'],
    retrievedAt: source['retrieved-at'],
    contentHash: source['content-hash'],
    replyTo: source['reply-to'],
    isForwarded: source['is-forwarded'],
    truncated: source['truncated'],
    partition: source['partition'],
  });
}

export function decodeStoredSourceDocument(data: string): StoredSourceDocument {
  if (new TextEncoder().encode(data).length > 32 * 1024 * 1024)
    throw new Error('Документ источника превышает 32 МиБ.');
  const envelope = fields(JSON.parse(data), ['format', 'version', 'document']);
  if (envelope['format'] !== 'proteus-source-document' || envelope['version'] !== 2)
    throw new Error('Неподдерживаемая версия документа источника.');
  return decodeSourceDocument(envelope['document']);
}

export function encodeStoredSourceDocument(document: StoredSourceDocument): string {
  const data = JSON.stringify({ format: 'proteus-source-document', version: 2, document });
  decodeStoredSourceDocument(data);
  return data;
}
