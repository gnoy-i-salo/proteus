import type { MemoryItemDraft, SourceExcerpt, SourceSpan } from '@proteus-int/contracts';

// Host-owned binding of one bounded extraction attempt. Model output never
// supplies episode IDs, persisted IDs, timestamps, policy or versions.
export interface MemoryAnalysisSource {
  readonly episodeId: string;
  readonly excerpt: SourceExcerpt;
}

export interface MemoryAnalysisScope {
  readonly sources: readonly MemoryAnalysisSource[];
  readonly existingItemIds: readonly string[];
  readonly maxItems: number;
}

// Two cap groups bound one extraction. Source-input caps guard the untrusted
// host scope before large allocation: at most 100 excerpts, 1000 existing
// item ids, 64 KiB of UTF8 per excerpt and 1 MiB of excerpt bytes in total.
// Output caps bound accepted model items: at most maxItems (1..100) drafts,
// 8 spans per item, 8 KiB of item text, 64 KiB of item text in total,
// 8-entry lists, 1 KiB per limitation and 256 UTF8 bytes per id. Host
// presentation caps (30 evidence / 12 exemplars) are not enforced here: they
// bound later context assembly, not what this parser accepts.
const maxSources = 100;
const maxExistingItemIds = 1000;
const maxExcerptBytes = 65536; // 64 KiB of UTF8 per scope source excerpt.
const maxScopeExcerptBytes = 1048576; // 1 MiB of excerpt bytes across the scope.
const maxSpansPerItem = 8;
const maxItemTextBytes = 8192;
const maxTotalTextBytes = 65536; // 64 KiB across all returned item text.
const maxListEntries = 8;
const maxIdBytes = 256;
const maxLimitationChars = 1024;

const kinds = ['episode', 'reportedAssertion', 'procedure', 'styleExample'] as const;
const supports = ['observed', 'reported', 'hypothesis', 'unknown', 'conflicting'] as const;
const itemFields = [
  'draftId',
  'kind',
  'text',
  'sourceSpans',
  'eventAt',
  'support',
  'contradictions',
  'limitations',
] as const;

// Date.parse alone normalizes impossible dates (2026-02-30 becomes March), so
// calendar fields are range-checked explicitly before the timestamp is used.
const isoTimestamp =
  /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2}):(\d{2})(?:\.\d{1,9})?(Z|[+-]\d{2}:\d{2})$/u;

const encoder = new TextEncoder();
const utf8Decoder = new TextDecoder('utf-8', { fatal: true, ignoreBOM: true });

type Contradiction = MemoryItemDraft['contradictions'][number];

interface BoundSource {
  readonly episodeId: string;
  readonly excerpt: SourceExcerpt;
  readonly bytes: Uint8Array;
}

interface BoundScope {
  readonly byRevision: ReadonlyMap<string, readonly BoundSource[]>;
  readonly existingItemIds: ReadonlySet<string>;
}

interface ItemContext {
  readonly byRevision: ReadonlyMap<string, readonly BoundSource[]>;
  readonly draftIds: ReadonlySet<string>;
  readonly existingItemIds: ReadonlySet<string>;
}

interface ParsedSpan {
  readonly span: SourceSpan;
  readonly episodeId: string;
  readonly matching: readonly BoundSource[];
  readonly slice: string;
}

function fail(message: string): never {
  throw new Error(`STALKER: memory analysis: ${message}`);
}

// TextEncoder silently replaces lone surrogates with U+FFFD, so validity is
// proven by an exact encode/decode roundtrip rather than by decoding alone.
function assertValidUnicode(text: string, label: string): void {
  if (utf8Decoder.decode(encoder.encode(text)) !== text) fail(`${label} must be valid Unicode.`);
}

function fields(value: unknown, expected: readonly string[]): Record<string, unknown> {
  if (!value || typeof value !== 'object' || Array.isArray(value)) fail('expected an object.');
  const record = value as Record<string, unknown>;
  const keys = Object.keys(record);
  if (keys.length !== expected.length || keys.some((key) => !expected.includes(key)))
    fail(`unknown or missing fields; expected exactly ${expected.join(', ')}.`);
  return record;
}

function boundedText(value: unknown, label: string, maxBytes: number): string {
  if (typeof value !== 'string') fail(`${label} must be a string.`);
  if (value.length === 0) fail(`${label} must not be empty.`);
  // UTF8 never encodes a UTF-16 unit into fewer bytes, so an over-long string
  // is rejected before paying for the exact encoding.
  if (value.length > maxBytes) fail(`${label} exceeds ${maxBytes} UTF8 bytes.`);
  if (encoder.encode(value).length > maxBytes) fail(`${label} exceeds ${maxBytes} UTF8 bytes.`);
  assertValidUnicode(value, label);
  return value;
}

function boundedId(value: unknown, label: string): string {
  if (typeof value !== 'string') fail(`${label} must be a string.`);
  if (value.length === 0) fail(`${label} must not be empty.`);
  if (/^\s+$/u.test(value)) fail(`${label} must not be whitespace-only.`);
  if (/\p{Cc}/u.test(value)) fail(`${label} must not contain control characters.`);
  if (value.length > maxIdBytes) fail(`${label} exceeds ${maxIdBytes} UTF8 bytes.`);
  if (encoder.encode(value).length > maxIdBytes) fail(`${label} exceeds ${maxIdBytes} UTF8 bytes.`);
  assertValidUnicode(value, label);
  return value;
}

function safeOffset(value: unknown, label: string): number {
  if (typeof value !== 'number' || !Number.isSafeInteger(value) || value < 0)
    fail(`${label} must be a safe non-negative integer.`);
  return value;
}

function choice<T extends string>(value: unknown, options: readonly T[], label: string): T {
  if (typeof value !== 'string' || !(options as readonly string[]).includes(value))
    fail(`unknown ${label} value.`);
  return value as T;
}

function revisionKey(documentId: string, revisionId: string): string {
  return JSON.stringify([documentId, revisionId]);
}

function bindScopeSource(source: MemoryAnalysisSource): BoundSource {
  if (!source || typeof source !== 'object' || Array.isArray(source))
    fail('scope sources must be objects.');
  const episodeId = boundedId(source.episodeId, 'episode id');
  const excerpt = source.excerpt;
  if (!excerpt || typeof excerpt !== 'object' || Array.isArray(excerpt))
    fail('scope excerpt must be an object.');
  const span = excerpt.span;
  if (!span || typeof span !== 'object') fail('scope excerpt span must be an object.');
  if (span.unit !== 'utf8Bytes') fail('scope excerpt span unit must be utf8Bytes.');
  boundedId(span.documentId, 'scope excerpt document id');
  boundedId(span.revisionId, 'scope excerpt revision id');
  const start = safeOffset(span.start, 'scope excerpt span start');
  const end = safeOffset(span.end, 'scope excerpt span end');
  if (end <= start) fail('scope excerpt span end must exceed start.');
  const document = excerpt.document;
  if (!document || typeof document !== 'object') fail('scope excerpt document must be an object.');
  if (document.partition !== 'reference') fail('scope excerpt must be in the reference partition.');
  if (document.id !== span.documentId)
    fail('scope excerpt document id must equal the span document id.');
  if (typeof excerpt.text !== 'string') fail('scope excerpt text must be a string.');
  if (excerpt.text.length > maxExcerptBytes)
    fail(`scope excerpt text exceeds the ${maxExcerptBytes} UTF8 byte bound.`);
  const bytes = encoder.encode(excerpt.text);
  if (bytes.length !== end - start) fail('scope excerpt byte length must equal end - start.');
  if (bytes.length > maxExcerptBytes)
    fail(`scope excerpt text exceeds the ${maxExcerptBytes} UTF8 byte bound.`);
  assertValidUnicode(excerpt.text, 'scope excerpt text');
  return { episodeId, excerpt, bytes };
}

function bindScope(scope: MemoryAnalysisScope): BoundScope {
  if (!scope || typeof scope !== 'object' || Array.isArray(scope)) fail('scope must be an object.');
  if (!Number.isSafeInteger(scope.maxItems) || scope.maxItems < 1 || scope.maxItems > 100)
    fail('scope maxItems must be a safe integer in 1..100.');
  if (!Array.isArray(scope.sources)) fail('scope sources must be an array.');
  const sources: readonly MemoryAnalysisSource[] = scope.sources;
  if (sources.length > maxSources) fail(`scope sources exceed the ${maxSources} excerpt bound.`);
  if (!Array.isArray(scope.existingItemIds)) fail('scope existingItemIds must be an array.');
  if (scope.existingItemIds.length > maxExistingItemIds)
    fail(`scope existingItemIds exceed the ${maxExistingItemIds} bound.`);
  const existingItemIds = new Set<string>();
  for (const id of scope.existingItemIds) existingItemIds.add(boundedId(id, 'existing item id'));
  const byRevision = new Map<string, BoundSource[]>();
  let totalExcerptBytes = 0;
  for (const source of sources) {
    const bound = bindScopeSource(source);
    totalExcerptBytes += bound.bytes.length;
    if (totalExcerptBytes > maxScopeExcerptBytes)
      fail(`scope excerpts exceed the 1 MiB total UTF8 byte bound.`);
    const key = revisionKey(bound.excerpt.span.documentId, bound.excerpt.span.revisionId);
    const list = byRevision.get(key);
    if (list) list.push(bound);
    else byRevision.set(key, [bound]);
  }
  return { byRevision, existingItemIds };
}

function decodeSlice(source: BoundSource, start: number, end: number): string {
  const offset = source.excerpt.span.start;
  try {
    return utf8Decoder.decode(source.bytes.subarray(start - offset, end - offset));
  } catch {
    fail('span splits a multibyte character.');
  }
}

function parseSpan(
  raw: unknown,
  byRevision: ReadonlyMap<string, readonly BoundSource[]>,
): ParsedSpan {
  const input = fields(raw, ['documentId', 'revisionId', 'unit', 'start', 'end']);
  const documentId = boundedId(input.documentId, 'span document id');
  const revisionId = boundedId(input.revisionId, 'span revision id');
  if (input.unit !== 'utf8Bytes') fail('span unit must be utf8Bytes.');
  const start = safeOffset(input.start, 'span start');
  const end = safeOffset(input.end, 'span end');
  if (end <= start) fail('span end must exceed span start.');
  const candidates = byRevision.get(revisionKey(documentId, revisionId));
  if (!candidates || candidates.length === 0)
    fail('span references a document revision outside the supplied excerpts.');
  const matching = candidates.filter(
    (candidate) => start >= candidate.excerpt.span.start && end <= candidate.excerpt.span.end,
  );
  if (matching.length === 0) fail('span must be fully contained in the supplied excerpt interval.');
  const episodeIds = new Set(matching.map((candidate) => candidate.episodeId));
  if (episodeIds.size !== 1) fail('span maps to ambiguous episodes for one document revision.');
  // Every matching interval overlaps this span, so duplicated excerpts must
  // supply identical bytes; a conflicting duplicate is rejected, not masked
  // by taking the first candidate.
  const slices = matching.map((candidate) => decodeSlice(candidate, start, end));
  const slice = slices[0];
  if (slice === undefined || slices.some((candidate) => candidate !== slice))
    fail('overlapping excerpts supply conflicting text for one span.');
  const source = matching[0];
  if (!source) fail('span references a missing source.');
  return {
    span: { documentId, revisionId, unit: 'utf8Bytes', start, end },
    episodeId: source.episodeId,
    matching,
    slice,
  };
}

function parseSpans(
  value: unknown,
  byRevision: ReadonlyMap<string, readonly BoundSource[]>,
): readonly ParsedSpan[] {
  if (!Array.isArray(value) || value.length < 1 || value.length > maxSpansPerItem)
    fail(`sourceSpans must list 1..${maxSpansPerItem} spans.`);
  const spans = value.map((raw) => parseSpan(raw, byRevision));
  const seen = new Set(spans.map((parsed) => JSON.stringify(parsed.span)));
  if (seen.size !== spans.length) fail('duplicate source span in one item.');
  return spans;
}

function canAttributeOwnWords(source: BoundSource): boolean {
  const excerpt = source.excerpt;
  const document = excerpt.document;
  return (
    excerpt.canSupportOwnWords === true &&
    excerpt.speakerRelation === 'target' &&
    excerpt.role === 'own' &&
    document.authorship === 'owner' &&
    document.isForwarded === false &&
    !excerpt.relations.some((relation) => relation.kind === 'quote' || relation.kind === 'forward')
  );
}

function requireOwnWords(text: string, spans: readonly ParsedSpan[]): void {
  let exact = false;
  for (const parsed of spans) {
    // Mixed matching attribution (one own, another context/quoted/forwarded)
    // must not be upgraded by picking a convenient candidate.
    if (!parsed.matching.every(canAttributeOwnWords))
      fail('attributed exact words require an own target-authored unquoted source.');
    if (parsed.slice === text) exact = true;
  }
  if (!exact) fail('item text must equal one exact cited source slice.');
}

function isLeapYear(year: number): boolean {
  return year % 4 === 0 && (year % 100 !== 0 || year % 400 === 0);
}

function parseEventAt(value: unknown): string | null {
  if (value === null) return null;
  if (typeof value !== 'string') fail('eventAt must be null or an explicit ISO timestamp.');
  const match = isoTimestamp.exec(value);
  if (!match) fail('eventAt must be null or an explicit ISO timestamp.');
  const year = Number(match[1]);
  const month = Number(match[2]);
  const day = Number(match[3]);
  const hour = Number(match[4]);
  const minute = Number(match[5]);
  const second = Number(match[6]);
  const offset = match[7] ?? 'Z';
  const daysInMonth = [31, isLeapYear(year) ? 29 : 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
  if (month < 1 || month > 12) fail('eventAt month must be within 1..12.');
  if (day < 1 || day > (daysInMonth[month - 1] ?? 0))
    fail('eventAt day is outside the calendar month.');
  if (hour > 23) fail('eventAt hour must be within 0..23.');
  if (minute > 59) fail('eventAt minute must be within 0..59.');
  if (second > 59) fail('eventAt second must be within 0..59.');
  if (offset !== 'Z') {
    const offsetHour = Number(offset.slice(1, 3));
    const offsetMinute = Number(offset.slice(4, 6));
    if (offsetHour > 23 || offsetMinute > 59) fail('eventAt timezone offset is out of range.');
  }
  if (!Number.isFinite(Date.parse(value))) fail('eventAt must be a valid calendar timestamp.');
  return value;
}

function parseLimitations(value: unknown): readonly string[] {
  if (!Array.isArray(value) || value.length > maxListEntries)
    fail(`limitations exceed the ${maxListEntries} entry bound.`);
  return value.map((entry) => boundedText(entry, 'limitation', maxLimitationChars));
}

function parseContradictions(
  value: unknown,
  ownDraftId: string,
  draftIds: ReadonlySet<string>,
  existingItemIds: ReadonlySet<string>,
): readonly Contradiction[] {
  if (!Array.isArray(value) || value.length > maxListEntries)
    fail(`contradictions exceed the ${maxListEntries} entry bound.`);
  const seen = new Set<string>();
  const edges: Contradiction[] = [];
  for (const raw of value) {
    const input = fields(raw, ['kind', 'itemId']);
    const itemId = boundedId(input.itemId, 'contradiction target id');
    let edge: Contradiction;
    if (input.kind === 'draft') {
      if (itemId === ownDraftId) fail('a contradiction cannot target its own draft.');
      if (!draftIds.has(itemId)) fail('draft contradiction target is missing from the batch.');
      edge = { kind: 'draft', draftId: itemId };
    } else if (input.kind === 'published') {
      if (!existingItemIds.has(itemId))
        fail('published contradiction target is unknown to the host.');
      edge = { kind: 'published', itemId };
    } else {
      fail('contradiction kind must be draft or published.');
    }
    const key = JSON.stringify(edge);
    if (seen.has(key)) fail('duplicate contradiction edge.');
    seen.add(key);
    edges.push(edge);
  }
  return edges;
}

function deriveEpisodeIds(spans: readonly ParsedSpan[]): readonly string[] {
  const ids: string[] = [];
  for (const parsed of spans) if (!ids.includes(parsed.episodeId)) ids.push(parsed.episodeId);
  return ids;
}

function parseItem(raw: unknown, context: ItemContext): MemoryItemDraft {
  const input = fields(raw, itemFields);
  const draftId = boundedId(input.draftId, 'draft id');
  const kind = choice(input.kind, kinds, 'kind');
  const support = choice(input.support, supports, 'support');
  if (kind === 'reportedAssertion' && support === 'observed')
    fail('a reportedAssertion cannot claim observed support.');
  const text = boundedText(input.text, 'item text', maxItemTextBytes);
  const spans = parseSpans(input.sourceSpans, context.byRevision);
  if (kind === 'styleExample' || support === 'observed') requireOwnWords(text, spans);
  return {
    draftId,
    kind,
    support,
    text,
    sourceSpans: spans.map((parsed) => parsed.span),
    episodeIds: deriveEpisodeIds(spans),
    eventAt: parseEventAt(input.eventAt),
    contradictions: parseContradictions(
      input.contradictions,
      draftId,
      context.draftIds,
      context.existingItemIds,
    ),
    limitations: parseLimitations(input.limitations),
  };
}

function collectDraftIds(raws: readonly unknown[]): ReadonlySet<string> {
  const draftIds = new Set<string>();
  for (const raw of raws) {
    if (!raw || typeof raw !== 'object' || Array.isArray(raw)) fail('each item must be an object.');
    const record = raw as Record<string, unknown>;
    if (!('draftId' in record)) fail('item is missing draftId.');
    const draftId = boundedId(record.draftId, 'draft id');
    if (draftIds.has(draftId)) fail('duplicate draft id.');
    draftIds.add(draftId);
  }
  return draftIds;
}

/**
 * Structural validation of untrusted model output against a host-owned scope.
 * This proves shape, bounds and cited slices only; it is not authorization,
 * publication, or a truth claim about the cited events.
 */
export function parseMemoryAnalysis(
  raw: unknown,
  scope: MemoryAnalysisScope,
): readonly MemoryItemDraft[] {
  const bound = bindScope(scope);
  const input = fields(raw, ['items']);
  if (!Array.isArray(input.items)) fail('items must be an array.');
  if (input.items.length > scope.maxItems)
    fail('response exceeds the host maxItems bound; the whole result is rejected.');
  const context: ItemContext = {
    byRevision: bound.byRevision,
    draftIds: collectDraftIds(input.items),
    existingItemIds: bound.existingItemIds,
  };
  let totalTextBytes = 0;
  const drafts: MemoryItemDraft[] = [];
  for (const rawItem of input.items) {
    const draft = parseItem(rawItem, context);
    totalTextBytes += encoder.encode(draft.text).length;
    if (totalTextBytes > maxTotalTextBytes)
      fail('response text exceeds the 64 KiB total bound; the whole result is rejected.');
    drafts.push(draft);
  }
  return drafts;
}
