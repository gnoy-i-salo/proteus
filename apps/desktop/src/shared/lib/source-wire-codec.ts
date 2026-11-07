import { decodeLegacySourceDocument } from '@proteus-int/contracts';
import type { SourceOperation, SourceProvider, StoredSourceDocument } from '@proteus-int/contracts';

// Preparatory IPC shapes; no consumers switch until the atomic domain migration.
export interface NativeSourceBudget {
  readonly maxSteps: number;
  readonly maxDocuments: number;
  readonly maxBytes: number;
  readonly maxDurationMs: number;
}
export interface NativeSourceGrantRequest {
  readonly provider: SourceProvider;
  readonly resourceId: string;
  readonly authorId?: string | null;
  readonly from?: string | null;
  readonly to?: string | null;
  readonly allowModelProcessing: boolean;
  readonly operations: readonly SourceOperation[];
  readonly budget: NativeSourceBudget;
}
export interface NativeSourceGrant extends NativeSourceGrantRequest {
  readonly authorId: string | null;
  readonly from: string | null;
  readonly to: string | null;
  readonly id: string;
  readonly accountId: string;
  readonly snapshotId: string;
  readonly expiresAt: string;
}
export type NativeSourceAction =
  | { readonly action: 'describe' | 'coverage' }
  | {
      readonly action: 'search';
      readonly query: string;
      readonly limit: number;
    }
  | {
      readonly action: 'collect';
      readonly cursor?: string | null;
      readonly limit: number;
    }
  | { readonly action: 'read'; readonly documentIds: readonly string[] };
export interface NativeSourceCoverage {
  readonly documents: number;
  readonly totalDocuments: number | null;
  readonly bytes: number;
  readonly complete: boolean;
  readonly truncated: boolean;
  readonly notAnalyzed: number;
  readonly unavailable: readonly string[];
}
export interface NativeSourceResult {
  readonly snapshotId: string;
  readonly documents: readonly StoredSourceDocument[];
  readonly coverage: NativeSourceCoverage;
  readonly cursor: string | null;
  readonly limitations: readonly string[];
}

function fields(value: unknown, allowed: readonly string[]): Record<string, unknown> {
  if (
    !value ||
    typeof value !== 'object' ||
    Array.isArray(value) ||
    Object.keys(value).some((key) => !allowed.includes(key))
  )
    throw new Error('Неверная схема источника.');
  return value as Record<string, unknown>;
}
function text(value: unknown): string {
  if (typeof value !== 'string') throw new Error('Неверное текстовое поле источника.');
  return value;
}
function nullableText(value: unknown): string | null {
  return value === null ? null : text(value);
}
function count(value: unknown, maximum = Number.MAX_SAFE_INTEGER): number {
  if (typeof value !== 'number' || !Number.isSafeInteger(value) || value < 0 || value > maximum)
    throw new Error('Неверный счётчик источника.');
  return value;
}
function flag(value: unknown): boolean {
  if (typeof value !== 'boolean') throw new Error('Неверный флаг источника.');
  return value;
}
function array<T>(value: unknown, decode: (item: unknown) => T): T[] {
  if (!Array.isArray(value)) throw new Error('Неверный список источника.');
  return value.map(decode);
}
function choice<T extends string>(value: unknown, choices: readonly T[]): T {
  const selected = choices.find((item) => item === value);
  if (selected === undefined) throw new Error('Неизвестное значение поля источника.');
  return selected;
}
function budget(value: unknown, legacy: boolean): NativeSourceBudget {
  const source = fields(
    value,
    legacy
      ? ['max-steps', 'max-documents', 'max-bytes', 'max-duration-ms']
      : ['maxSteps', 'maxDocuments', 'maxBytes', 'maxDurationMs'],
  );
  return {
    maxSteps: count(source[legacy ? 'max-steps' : 'maxSteps'], 0xffffffff),
    maxDocuments: count(source[legacy ? 'max-documents' : 'maxDocuments'], 0xffffffff),
    maxBytes: count(source[legacy ? 'max-bytes' : 'maxBytes']),
    maxDurationMs: count(source[legacy ? 'max-duration-ms' : 'maxDurationMs']),
  };
}
function request(
  value: unknown,
  legacy: boolean,
  extra: readonly string[] = [],
): NativeSourceGrantRequest {
  const source = fields(value, [
    'provider',
    'from',
    'to',
    'operations',
    'budget',
    ...extra,
    ...(legacy
      ? ['resource-id', 'author-id', 'allow-model-processing']
      : ['resourceId', 'authorId', 'allowModelProcessing']),
  ]);
  const authorKey = legacy ? 'author-id' : 'authorId';
  return {
    provider: choice(source.provider, ['telegram', 'vk', 'instagram', 'github'] as const),
    resourceId: text(source[legacy ? 'resource-id' : 'resourceId']),
    ...(Object.hasOwn(source, authorKey) ? { authorId: nullableText(source[authorKey]) } : {}),
    ...(Object.hasOwn(source, 'from') ? { from: nullableText(source.from) } : {}),
    ...(Object.hasOwn(source, 'to') ? { to: nullableText(source.to) } : {}),
    allowModelProcessing: flag(source[legacy ? 'allow-model-processing' : 'allowModelProcessing']),
    operations: array(source.operations, (value) =>
      choice(value, ['describe', 'search', 'collect', 'read', 'coverage'] as const),
    ),
    budget: budget(source.budget, legacy),
  };
}

export function decodeLegacySourceGrantRequest(value: unknown): NativeSourceGrantRequest {
  return request(value, true);
}

// Conversion never checks or grants authorization; the native host still owns access decisions.
export function decodeLegacySourceGrant(value: unknown): NativeSourceGrant {
  const base = request(value, true, ['id', 'account-id', 'snapshot-id', 'expires-at']);
  const source = value as Record<string, unknown>;
  return {
    ...base,
    authorId: nullableText(source['author-id']),
    from: nullableText(source.from),
    to: nullableText(source.to),
    id: text(source.id),
    accountId: text(source['account-id']),
    snapshotId: text(source['snapshot-id']),
    expiresAt: text(source['expires-at']),
  };
}

export function encodeLegacySourceGrantRequest(value: unknown) {
  const source = request(value, false);
  return {
    provider: source.provider,
    'resource-id': source.resourceId,
    ...(Object.hasOwn(source, 'authorId') ? { 'author-id': source.authorId } : {}),
    ...(Object.hasOwn(source, 'from') ? { from: source.from } : {}),
    ...(Object.hasOwn(source, 'to') ? { to: source.to } : {}),
    'allow-model-processing': source.allowModelProcessing,
    operations: [...source.operations],
    budget: {
      'max-steps': source.budget.maxSteps,
      'max-documents': source.budget.maxDocuments,
      'max-bytes': source.budget.maxBytes,
      'max-duration-ms': source.budget.maxDurationMs,
    },
  };
}

function action(value: unknown, legacy: boolean): NativeSourceAction {
  if (!value || typeof value !== 'object' || Array.isArray(value))
    throw new Error('Неверное действие источника.');
  const kind = choice((value as Record<string, unknown>).action, [
    'describe',
    'coverage',
    'search',
    'collect',
    'read',
  ] as const);
  if (kind === 'describe' || kind === 'coverage') {
    fields(value, ['action']);
    return { action: kind };
  }
  if (kind === 'search') {
    const source = fields(value, ['action', 'query', 'limit']);
    return {
      action: kind,
      query: text(source.query),
      limit: count(source.limit),
    };
  }
  if (kind === 'collect') {
    const source = fields(value, ['action', 'cursor', 'limit']);
    return {
      action: kind,
      limit: count(source.limit),
      ...(Object.hasOwn(source, 'cursor') ? { cursor: nullableText(source.cursor) } : {}),
    };
  }
  const key = legacy ? 'document-ids' : 'documentIds';
  const source = fields(value, ['action', key]);
  return { action: kind, documentIds: array(source[key], text) };
}
export function decodeLegacySourceAction(value: unknown): NativeSourceAction {
  return action(value, true);
}
export function encodeLegacySourceAction(value: unknown) {
  const source = action(value, false);
  if (source.action === 'read')
    return { action: source.action, 'document-ids': [...source.documentIds] };
  return { ...source };
}

export function decodeLegacySourceCoverage(value: unknown): NativeSourceCoverage {
  const source = fields(value, [
    'documents',
    'total-documents',
    'bytes',
    'complete',
    'truncated',
    'not-analyzed',
    'unavailable',
  ]);
  return {
    documents: count(source.documents),
    totalDocuments: source['total-documents'] === null ? null : count(source['total-documents']),
    bytes: count(source.bytes),
    complete: flag(source.complete),
    truncated: flag(source.truncated),
    notAnalyzed: count(source['not-analyzed']),
    unavailable: array(source.unavailable, text),
  };
}

export function decodeLegacySourceResult(value: unknown): NativeSourceResult {
  const source = fields(value, ['snapshot-id', 'documents', 'coverage', 'cursor', 'limitations']);
  // Same upper bound as the native plaintext corpus; no serialization is retained or exported.
  if (new TextEncoder().encode(JSON.stringify(source)).length > 32 * 1024 * 1024)
    throw new Error('Ответ источника превышает 32 МиБ.');
  return {
    snapshotId: text(source['snapshot-id']),
    documents: array(source.documents, decodeLegacySourceDocument),
    coverage: decodeLegacySourceCoverage(source.coverage),
    cursor: nullableText(source.cursor),
    limitations: array(source.limitations, text),
  };
}
