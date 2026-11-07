import { decodeSourceDocument } from '@proteus-int/contracts';
import type {
  SourceBinding,
  SourceBindingRequest,
  CreatedSourceBinding,
  SourceBindingResult,
  SourceGrantRequest,
  SourceAction,
  ModelSourceDocument,
  DocumentRelation,
  SourceSpan,
} from '@proteus-int/contracts';
import {
  decodeSourceGrant,
  decodeSourceResult,
  encodeSourceGrantRequest,
  encodeSourceAction,
} from './source-wire-codec';

function fields(value: unknown, keys: readonly string[]): Record<string, unknown> {
  if (
    !value ||
    typeof value !== 'object' ||
    Array.isArray(value) ||
    Object.keys(value).length !== keys.length ||
    Object.keys(value).some((key) => !keys.includes(key))
  )
    throw new Error('Неверная схема привязки источника.');
  return value as Record<string, unknown>;
}
function text(value: unknown): string {
  if (typeof value !== 'string') throw new Error('Неверное текстовое поле источника.');
  return value;
}
function nullableText(value: unknown): string | null {
  return value === null ? null : text(value);
}
function flag(value: unknown): boolean {
  if (typeof value !== 'boolean') throw new Error('Неверный флаг привязки источника.');
  return value;
}
function choice<T extends string>(value: unknown, allowed: readonly T[]): T {
  const selected = allowed.find((item) => item === value);
  if (selected === undefined) throw new Error('Неизвестное поле привязки источника.');
  return selected;
}
function identifier(value: unknown): string {
  const id = text(value);
  if (!id.trim() || new TextEncoder().encode(id).length > 128 || /\p{Cc}/u.test(id))
    throw new Error('Неверный идентификатор привязки источника.');
  return id;
}
function contextIds(value: unknown, targetId: string | null): string[] {
  if (!Array.isArray(value) || value.length > 128) throw new Error('Выберите до 128 собеседников.');
  const ids = value.map(identifier);
  if (new Set(ids).size !== ids.length || (targetId !== null && ids.includes(targetId)))
    throw new Error('Идентификаторы авторов не должны повторяться.');
  return ids;
}
function sourceRequest(value: unknown): SourceGrantRequest {
  fields(value, [
    'provider',
    'resourceId',
    'authorId',
    'from',
    'to',
    'allowModelProcessing',
    'operations',
    'budget',
  ]);
  const source = encodeSourceGrantRequest(value);
  return {
    ...source,
    authorId: nullableText(source.authorId),
    from: nullableText(source.from),
    to: nullableText(source.to),
  };
}

export function encodeSourceBindingRequest(value: unknown): SourceBindingRequest {
  const input = fields(value, [
    'source',
    'personaId',
    'role',
    'allowedContextAuthorIds',
    'allowUnknownContext',
    'includeUndated',
  ]);
  const source = sourceRequest(input.source);
  const role = choice(input.role, ['own', 'context', 'about', 'reference'] as const);
  if (source.authorId !== null) identifier(source.authorId);
  if (role === 'own' && source.authorId === null) throw new Error('Выберите целевого автора.');
  return {
    source,
    personaId: identifier(input.personaId),
    role,
    allowedContextAuthorIds: contextIds(input.allowedContextAuthorIds, source.authorId),
    allowUnknownContext: flag(input.allowUnknownContext),
    includeUndated: flag(input.includeUndated),
  };
}
function binding(value: unknown): SourceBinding {
  const input = fields(value, [
    'schemaVersion',
    'id',
    'personaId',
    'provider',
    'accountId',
    'resourceId',
    'grantId',
    'policyVersion',
    'role',
    'targetAuthorId',
    'allowedContextAuthorIds',
    'allowUnknownContext',
    'period',
  ]);
  if (input.schemaVersion !== 1) throw new Error('Неизвестная версия привязки источника.');
  const period = fields(input.period, ['from', 'to', 'includeUndated']);
  const targetAuthorId = input.targetAuthorId === null ? null : identifier(input.targetAuthorId);
  const role = choice(input.role, ['own', 'context', 'about', 'reference'] as const);
  if (role === 'own' && targetAuthorId === null) throw new Error('В привязке отсутствует автор.');
  return {
    schemaVersion: 1,
    id: identifier(input.id),
    personaId: identifier(input.personaId),
    provider: choice(input.provider, [
      'telegram',
      'vk',
      'instagram',
      'github',
      'web',
      'savedPage',
    ] as const),
    accountId: nullableText(input.accountId),
    resourceId: text(input.resourceId),
    grantId: identifier(input.grantId),
    policyVersion: text(input.policyVersion),
    role,
    targetAuthorId,
    allowedContextAuthorIds: contextIds(input.allowedContextAuthorIds, targetAuthorId),
    allowUnknownContext: flag(input.allowUnknownContext),
    period: {
      from: nullableText(period.from),
      to: nullableText(period.to),
      includeUndated: flag(period.includeUndated),
    },
  };
}
export function decodeCreatedSourceBinding(
  value: unknown,
  request: SourceBindingRequest,
): CreatedSourceBinding {
  const input = fields(value, ['binding', 'grant']);
  const selected = binding(input.binding);
  const grant = decodeSourceGrant(input.grant);
  const grantedSource = sourceRequest({
    provider: grant.provider,
    resourceId: grant.resourceId,
    authorId: grant.authorId,
    from: grant.from,
    to: grant.to,
    allowModelProcessing: grant.allowModelProcessing,
    operations: grant.operations,
    budget: grant.budget,
  });
  if (
    selected.grantId !== grant.id ||
    selected.provider !== grant.provider ||
    selected.accountId !== grant.accountId ||
    selected.resourceId !== grant.resourceId ||
    selected.personaId !== request.personaId ||
    selected.role !== request.role ||
    selected.targetAuthorId !== request.source.authorId ||
    selected.allowUnknownContext !== request.allowUnknownContext ||
    selected.period.includeUndated !== request.includeUndated ||
    selected.period.from !== request.source.from ||
    selected.period.to !== request.source.to ||
    selected.allowedContextAuthorIds.length !== request.allowedContextAuthorIds.length ||
    selected.allowedContextAuthorIds.some((id) => !request.allowedContextAuthorIds.includes(id)) ||
    // Native resolves public handles and URLs to its canonical resource ID.
    JSON.stringify({ ...grantedSource, resourceId: request.source.resourceId }) !==
      JSON.stringify(request.source)
  )
    throw new Error('Ответ не соответствует выбранной привязке источника.');
  return { binding: selected, grant };
}
function span(value: unknown): SourceSpan {
  const input = fields(value, ['documentId', 'revisionId', 'unit', 'start', 'end']);
  if (
    input.unit !== 'utf8Bytes' ||
    !Number.isSafeInteger(input.start) ||
    !Number.isSafeInteger(input.end) ||
    Number(input.start) < 0 ||
    Number(input.end) <= Number(input.start)
  )
    throw new Error('Неверный диапазон исходного текста.');
  return {
    documentId: text(input.documentId),
    revisionId: text(input.revisionId),
    unit: 'utf8Bytes',
    start: Number(input.start),
    end: Number(input.end),
  };
}
function relation(value: unknown): DocumentRelation {
  const input = fields(value, ['kind', 'original', 'originalAuthor', 'span']);
  const original =
    input.original === null ? null : fields(input.original, ['documentId', 'revisionId']);
  const author = fields(input.originalAuthor, ['provider', 'id', 'label']);
  return {
    kind: choice(input.kind, ['reply', 'quote', 'forward'] as const),
    original:
      original === null
        ? null
        : { documentId: text(original.documentId), revisionId: text(original.revisionId) },
    originalAuthor: {
      provider:
        author.provider === null
          ? null
          : choice(author.provider, [
              'telegram',
              'vk',
              'instagram',
              'github',
              'web',
              'savedPage',
            ] as const),
      id: nullableText(author.id),
      label: nullableText(author.label),
    },
    span: input.span === null ? null : span(input.span),
  };
}
function projection(value: unknown): ModelSourceDocument {
  const input = fields(value, [
    'document',
    'revisionId',
    'role',
    'speakerRelation',
    'canSupportOwnWords',
    'relations',
  ]);
  const document = decodeSourceDocument(input.document);
  const role = choice(input.role, ['own', 'context', 'about', 'reference'] as const);
  const speakerRelation = choice(input.speakerRelation, ['target', 'context', 'unknown'] as const);
  const canSupportOwnWords = flag(input.canSupportOwnWords);
  if (!Array.isArray(input.relations)) throw new Error('Неверные отношения источника.');
  const relations = input.relations.map(relation);
  if (
    document.partition !== 'reference' ||
    (document.authorId === null && speakerRelation !== 'unknown') ||
    (canSupportOwnWords &&
      (role !== 'own' ||
        speakerRelation !== 'target' ||
        document.authorship !== 'owner' ||
        document.isForwarded ||
        relations.some((item) => item.kind === 'quote' || item.kind === 'forward')))
  )
    throw new Error('Атрибуция исходного текста противоречит метаданным.');
  return {
    document,
    revisionId: identifier(input.revisionId),
    role,
    speakerRelation,
    canSupportOwnWords,
    relations,
  };
}
export function decodeSourceBindingResult(value: unknown): SourceBindingResult {
  const input = fields(value, ['snapshotId', 'documents', 'coverage', 'cursor', 'limitations']);
  if (new TextEncoder().encode(JSON.stringify(input)).length > 32 * 1024 * 1024)
    throw new Error('Ответ источника превышает 32 МиБ.');
  if (!Array.isArray(input.documents) || input.documents.length > 20)
    throw new Error('Неверный список документов источника.');
  const documents = input.documents.map(projection);
  const result = decodeSourceResult({
    ...input,
    documents: documents.map((item) => item.document),
  });
  if (result.cursor !== null && result.coverage.complete)
    throw new Error('Неверная полнота источника.');
  return { ...result, documents };
}
export function encodeBindingAction(value: unknown): SourceAction {
  const action = encodeSourceAction(value);
  if (action.action === 'search') {
    if (
      !action.query.trim() ||
      new TextEncoder().encode(action.query).length > 256 ||
      action.limit < 1 ||
      action.limit > 20
    )
      throw new Error('Неверный поисковый запрос источника.');
    return action;
  }
  if (action.action === 'collect') {
    if (
      action.limit < 1 ||
      action.limit > 20 ||
      action.cursor === undefined ||
      (action.cursor !== null && new TextEncoder().encode(action.cursor).length > 1024)
    )
      throw new Error('Неверная страница источника.');
    return { ...action, cursor: action.cursor };
  }
  if (action.action === 'read') {
    if (
      action.documentIds.length < 1 ||
      action.documentIds.length > 20 ||
      new Set(action.documentIds).size !== action.documentIds.length
    )
      throw new Error('Выберите от 1 до 20 разных документов.');
    action.documentIds.forEach(identifier);
  }
  return action;
}
export function encodeBindingId(value: unknown): string {
  return identifier(value);
}
