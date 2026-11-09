import type {
  CorpusIndexReceipt,
  CorpusSearchHit,
  CorpusSearchQuery,
  CorpusSearchResult,
} from './corpus-search';

// Strict decoders for the native reference-only FTS boundary. They check exact
// key sets and exact schemaVersion 1, reject unknown fields, non-finite,
// fractional or unsafe numbers and foreign response hits, and never rename or
// normalize payload values. Native fetches limit + 1 rows and truncates to the
// requested limit, so a true hasMore always ships a nonempty page. They are
// structural only: the native runtime re-authorizes grants and scope on every
// call.

const MAX_JOB_ID_BYTES = 128;
const MAX_ID_BYTES = 256;
const MAX_QUERY_TEXT_BYTES = 4096;
const MAX_POLICY_FINGERPRINT_BYTES = 256;
const MAX_LIMIT = 100;
const MAX_OFFSET = 10_000;
const MAX_SEARCH_DURATION_MS = 60_000;
const MAX_SPAN = 1_000_000_000_000;
const MAX_HITS = 100;
const MAX_CHUNK_COUNT = 1_000_000;
// Native Duration::as_secs() returns u64. Keep the decimal string exact.
const UNIX_SECONDS_PATTERN = /^\d{1,20}$/;
// Native producers render lowercase hex digests; uppercase is rejected here.
const SHA256_HEX_PATTERN = /^[0-9a-f]{64}$/;

const encoder = new TextEncoder();

interface SearchScope {
  readonly snapshotId: string;
  readonly indexGeneration: number;
}

function fields(value: unknown, keys: readonly string[]): Record<string, unknown> {
  if (!value || typeof value !== 'object' || Array.isArray(value))
    throw new Error('Неверная схема поиска корпуса.');
  const actual = Object.keys(value);
  if (actual.length !== keys.length || actual.some((key) => !keys.includes(key)))
    throw new Error('Отсутствующее или неизвестное поле поиска корпуса.');
  return value as Record<string, unknown>;
}

function utf8Bytes(value: string): number {
  return encoder.encode(value).length;
}

function searchId(value: unknown, maxBytes: number): string {
  if (
    typeof value !== 'string' ||
    !value.trim() ||
    utf8Bytes(value) > maxBytes ||
    /\p{Cc}/u.test(value)
  )
    throw new Error('Недопустимый идентификатор поиска корпуса.');
  return value;
}

function nullableSearchId(value: unknown): string | null {
  return value === null ? null : searchId(value, MAX_ID_BYTES);
}

function integer(value: unknown): number {
  if (typeof value !== 'number' || !Number.isSafeInteger(value))
    throw new Error('Недопустимое целое число поиска корпуса.');
  return value;
}

function positiveInteger(value: unknown): number {
  const result = integer(value);
  if (result < 1) throw new Error('Поколение поиска корпуса должно быть положительным.');
  return result;
}

function boundedInteger(value: unknown, minimum: number, maximum: number): number {
  const result = integer(value);
  if (result < minimum || result > maximum)
    throw new Error('Целое число поиска корпуса вне допустимого диапазона.');
  return result;
}

function flag(value: unknown): boolean {
  if (typeof value !== 'boolean') throw new Error('Недопустимый флаг поиска корпуса.');
  return value;
}

function finiteNumber(value: unknown): number {
  if (typeof value !== 'number' || !Number.isFinite(value))
    throw new Error('Недопустимое число поиска корпуса.');
  return value;
}

// Byte-exact only: no trimming and no Unicode normalization of queryText.
function exactQueryText(value: unknown): string {
  const bytes = typeof value === 'string' ? utf8Bytes(value) : 0;
  if (typeof value !== 'string' || bytes < 1 || bytes > MAX_QUERY_TEXT_BYTES)
    throw new Error('Поисковый запрос пуст или превышает допустимый размер.');
  return value;
}

function searchPeriod(value: unknown): CorpusSearchQuery['period'] {
  const input = fields(value, ['from', 'to', 'includeUndated']);
  const from = input.from === null ? null : integer(input.from);
  const to = input.to === null ? null : integer(input.to);
  if (from !== null && to !== null && from > to) throw new Error('Неверный период поиска корпуса.');
  return { from, to, includeUndated: flag(input.includeUndated) };
}

export function decodeCorpusSearchQuery(value: unknown): CorpusSearchQuery {
  const input = fields(value, [
    'jobId',
    'snapshotId',
    'personaId',
    'queryText',
    'period',
    'limit',
    'offset',
    'expectedIndexGeneration',
    'maxDurationMs',
  ]);
  const offset = boundedInteger(input.offset, 0, MAX_OFFSET);
  const expectedIndexGeneration =
    input.expectedIndexGeneration === null ? null : positiveInteger(input.expectedIndexGeneration);
  if (offset > 0 && expectedIndexGeneration === null)
    throw new Error('Для постраничного поиска корпуса требуется поколение индекса.');
  return {
    jobId: searchId(input.jobId, MAX_JOB_ID_BYTES),
    snapshotId: searchId(input.snapshotId, MAX_ID_BYTES),
    personaId: searchId(input.personaId, MAX_ID_BYTES),
    queryText: exactQueryText(input.queryText),
    period: searchPeriod(input.period),
    limit: boundedInteger(input.limit, 1, MAX_LIMIT),
    offset,
    expectedIndexGeneration,
    maxDurationMs: boundedInteger(input.maxDurationMs, 1, MAX_SEARCH_DURATION_MS),
  };
}

function searchHit(value: unknown, scope: SearchScope): CorpusSearchHit {
  const input = fields(value, [
    'chunkId',
    'snapshotId',
    'indexGeneration',
    'bm25',
    'documentId',
    'revisionId',
    'spanStart',
    'spanEnd',
    'personaId',
    'grantId',
    'bindingId',
    'periodFrom',
    'periodTo',
    'conversationId',
    'threadId',
  ]);
  const spanStart = boundedInteger(input.spanStart, 0, MAX_SPAN);
  const spanEnd = boundedInteger(input.spanEnd, 0, MAX_SPAN);
  if (spanEnd <= spanStart) throw new Error('Неверные границы чанка поиска корпуса.');
  const hit: CorpusSearchHit = {
    chunkId: searchId(input.chunkId, MAX_ID_BYTES),
    snapshotId: searchId(input.snapshotId, MAX_ID_BYTES),
    indexGeneration: positiveInteger(input.indexGeneration),
    bm25: finiteNumber(input.bm25),
    documentId: searchId(input.documentId, MAX_ID_BYTES),
    revisionId: searchId(input.revisionId, MAX_ID_BYTES),
    spanStart,
    spanEnd,
    personaId: searchId(input.personaId, MAX_ID_BYTES),
    grantId: searchId(input.grantId, MAX_ID_BYTES),
    bindingId: searchId(input.bindingId, MAX_ID_BYTES),
    periodFrom: input.periodFrom === null ? null : integer(input.periodFrom),
    periodTo: input.periodTo === null ? null : integer(input.periodTo),
    conversationId: nullableSearchId(input.conversationId),
    threadId: nullableSearchId(input.threadId),
  };
  if (hit.snapshotId !== scope.snapshotId || hit.indexGeneration !== scope.indexGeneration)
    throw new Error('Результат поиска корпуса содержит чужой снимок или поколение индекса.');
  if (hit.periodFrom !== null && hit.periodTo !== null && hit.periodFrom > hit.periodTo)
    throw new Error('Неверный период чанка поиска корпуса.');
  return hit;
}

export function decodeCorpusSearchResult(value: unknown): CorpusSearchResult {
  const input = fields(value, [
    'schemaVersion',
    'snapshotId',
    'snapshotGeneration',
    'indexGeneration',
    'hasMore',
    'hits',
  ]);
  if (input.schemaVersion !== 1) throw new Error('Неизвестная версия схемы поиска корпуса.');
  const snapshotId = searchId(input.snapshotId, MAX_ID_BYTES);
  const indexGeneration = positiveInteger(input.indexGeneration);
  if (!Array.isArray(input.hits) || input.hits.length > MAX_HITS)
    throw new Error('Слишком много результатов поиска корпуса.');
  const chunkIds = new Set<string>();
  const hits = input.hits.map((item) => {
    const hit = searchHit(item, { snapshotId, indexGeneration });
    if (chunkIds.has(hit.chunkId))
      throw new Error('Повторяющийся идентификатор чанка в результате поиска корпуса.');
    chunkIds.add(hit.chunkId);
    return hit;
  });
  const hasMore = flag(input.hasMore);
  if (hasMore && hits.length === 0)
    throw new Error('Пустая страница поиска корпуса не может иметь продолжение.');
  return {
    schemaVersion: 1,
    snapshotId,
    snapshotGeneration: positiveInteger(input.snapshotGeneration),
    indexGeneration,
    hasMore,
    hits,
  };
}

function policyFingerprint(value: unknown): string {
  if (
    typeof value !== 'string' ||
    !value.length ||
    utf8Bytes(value) > MAX_POLICY_FINGERPRINT_BYTES ||
    /\p{Cc}/u.test(value)
  )
    throw new Error('Недопустимый отпечаток политики индекса.');
  return value;
}

function contentSha256(value: unknown): string {
  if (typeof value !== 'string' || !SHA256_HEX_PATTERN.test(value))
    throw new Error('Недопустимый SHA256 манифеста индекса.');
  return value;
}

// Native `createdAt` is the audit UNIX-seconds string from now_seconds() at
// publication time: exact digits, never ISO8601 and never an original-source
// timestamp. Check the u64 bound without converting to a lossy JS number.
function unixSeconds(value: unknown): string {
  if (
    typeof value !== 'string' ||
    !UNIX_SECONDS_PATTERN.test(value) ||
    BigInt(value) > 18446744073709551615n
  )
    throw new Error('Недопустимое время публикации индекса.');
  return value;
}

export function decodeCorpusIndexReceipt(value: unknown): CorpusIndexReceipt {
  const input = fields(value, [
    'schemaVersion',
    'snapshotId',
    'snapshotGeneration',
    'indexGeneration',
    'policyFingerprint',
    'contentSha256',
    'chunkCount',
    'createdAt',
  ]);
  if (input.schemaVersion !== 1) throw new Error('Неизвестная версия квитанции индекса.');
  return {
    schemaVersion: 1,
    snapshotId: searchId(input.snapshotId, MAX_ID_BYTES),
    snapshotGeneration: positiveInteger(input.snapshotGeneration),
    indexGeneration: positiveInteger(input.indexGeneration),
    policyFingerprint: policyFingerprint(input.policyFingerprint),
    contentSha256: contentSha256(input.contentSha256),
    chunkCount: boundedInteger(input.chunkCount, 0, MAX_CHUNK_COUNT),
    createdAt: unixSeconds(input.createdAt),
  };
}
