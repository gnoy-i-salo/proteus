import type { MemoryItem, SkillDefinitionDraft, SourceSpan } from '@proteus-int/contracts';

// Host-owned binding of one bounded task-free skill extraction attempt. The
// host supplies the memory items of a single corpus snapshot and the skill
// budget; the model output never supplies ids, versions, permissions or tasks.
export interface MemorySkillScope {
  readonly items: readonly MemoryItem[];
  readonly maxSkills: number;
}

// Two cap groups bound one extraction. Scope-input caps guard the host scope
// before large allocation: at most 1000 memory items, 64 source spans and
// contradiction refs per item, 256 UTF8 bytes per id and 1 MiB of scope reference bytes in total.
// Output caps bound accepted skill drafts: at most maxSkills (1..10) drafts,
// 16 memory item refs and 16 spans per skill, 4 KiB of UTF8 for trigger and
// outcomeCheck, 2 KiB per inputs/steps/limitations entry, 16-entry lists and
// 64 KiB of skill text across the whole result. An oversized scope or output
// rejects the whole call; nothing is silently sliced.
const maxScopeItems = 1000;
const maxHostSpansPerItem = 64;
const maxHostContradictionRefs = 64;
const maxIdBytes = 256;
const maxScopeRefBytes = 1048576;
const maxRefsPerSkill = 16;
const maxSpansPerSkill = 16;
const maxListEntries = 16;
const maxFieldBytes = 4096;
const maxEntryBytes = 2048;
const maxTotalTextBytes = 65536;

const kinds = ['episode', 'reportedAssertion', 'procedure', 'styleExample'] as const;
const itemSupports = ['observed', 'reported', 'hypothesis', 'unknown', 'conflicting'] as const;
const skillSupports = ['observed', 'proposed'] as const;
const skillFields = [
  'draftId',
  'trigger',
  'inputs',
  'steps',
  'outcomeCheck',
  'support',
  'memoryItemIds',
  'sourceSpans',
  'limitations',
] as const;
const spanFields = ['documentId', 'revisionId', 'unit', 'start', 'end'] as const;

const encoder = new TextEncoder();
const utf8Decoder = new TextDecoder('utf-8', { fatal: true, ignoreBOM: true });

interface BoundItem {
  readonly id: string;
  readonly snapshotId: string;
  readonly kind: MemoryItem['kind'];
  readonly support: MemoryItem['support'];
  readonly contradictionIds: readonly string[];
  readonly sourceSpans: readonly SourceSpan[];
}

interface BoundScope {
  readonly byId: ReadonlyMap<string, BoundItem>;
}

interface SkillContext {
  readonly byId: ReadonlyMap<string, BoundItem>;
}

function fail(message: string): never {
  throw new Error(`FABRIC: memory skills: ${message}`);
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

function boundedText(value: unknown, label: string, maxBytes: number): string {
  if (typeof value !== 'string') fail(`${label} must be a string.`);
  if (value.length === 0) fail(`${label} must not be empty.`);
  if (/^\s+$/u.test(value)) fail(`${label} must not be blank.`);
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

function parseSpanShape(raw: unknown): SourceSpan {
  const input = fields(raw, spanFields);
  const documentId = boundedId(input.documentId, 'span document id');
  const revisionId = boundedId(input.revisionId, 'span revision id');
  if (input.unit !== 'utf8Bytes') fail('span unit must be utf8Bytes.');
  const start = safeOffset(input.start, 'span start');
  const end = safeOffset(input.end, 'span end');
  if (end <= start) fail('span end must exceed span start.');
  return { documentId, revisionId, unit: 'utf8Bytes', start, end };
}

function bindItem(raw: unknown): BoundItem {
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) fail('scope items must be objects.');
  const record = raw as Record<string, unknown>;
  if (record.schemaVersion !== 1) fail('unsupported memory item schema version.');
  const id = boundedId(record.id, 'memory item id');
  const snapshotId = boundedId(record.snapshotId, 'snapshot id');
  const kind = choice(record.kind, kinds, 'item kind');
  const support = choice(record.support, itemSupports, 'item support');
  if (!Array.isArray(record.contradictionIds)) fail('item contradictionIds must be an array.');
  if (record.contradictionIds.length > maxHostContradictionRefs)
    fail(`item contradictionIds exceed the ${maxHostContradictionRefs} entry bound.`);
  const contradictionIds = record.contradictionIds.map((entry: unknown) =>
    boundedId(entry, 'contradiction id'),
  );
  if (new Set(contradictionIds).size !== contradictionIds.length)
    fail('duplicate contradiction id in one item.');
  if (!Array.isArray(record.sourceSpans)) fail('item sourceSpans must be an array.');
  if (record.sourceSpans.length > maxHostSpansPerItem)
    fail(`item sourceSpans exceed the ${maxHostSpansPerItem} span bound.`);
  const sourceSpans = record.sourceSpans.map(parseSpanShape);
  return { id, snapshotId, kind, support, contradictionIds, sourceSpans };
}

function scopeRefBytes(bound: BoundItem): number {
  let total = encoder.encode(bound.id).length + encoder.encode(bound.snapshotId).length;
  for (const id of bound.contradictionIds) total += encoder.encode(id).length;
  for (const span of bound.sourceSpans)
    total += encoder.encode(span.documentId).length + encoder.encode(span.revisionId).length;
  return total;
}

function bindScope(scope: MemorySkillScope): BoundScope {
  if (!scope || typeof scope !== 'object' || Array.isArray(scope)) fail('scope must be an object.');
  if (!Number.isSafeInteger(scope.maxSkills) || scope.maxSkills < 1 || scope.maxSkills > 10)
    fail('scope maxSkills must be a safe integer in 1..10.');
  if (!Array.isArray(scope.items)) fail('scope items must be an array.');
  if (scope.items.length > maxScopeItems)
    fail(`scope items exceed the ${maxScopeItems} item bound.`);
  const byId = new Map<string, BoundItem>();
  let snapshotId: string | null = null;
  let refBytes = 0;
  for (const raw of scope.items) {
    const bound = bindItem(raw);
    if (byId.has(bound.id)) fail('duplicate memory item id in the scope.');
    if (snapshotId === null) snapshotId = bound.snapshotId;
    else if (snapshotId !== bound.snapshotId) fail('memory items must share one corpus snapshot.');
    refBytes += scopeRefBytes(bound);
    if (refBytes > maxScopeRefBytes)
      fail('scope references exceed the 1 MiB total UTF8 byte bound.');
    byId.set(bound.id, bound);
  }
  return { byId };
}

function collectDraftIds(raws: readonly unknown[]): void {
  const draftIds = new Set<string>();
  for (const raw of raws) {
    if (!raw || typeof raw !== 'object' || Array.isArray(raw))
      fail('each skill must be an object.');
    const record = raw as Record<string, unknown>;
    if (!('draftId' in record)) fail('skill is missing draftId.');
    const draftId = boundedId(record.draftId, 'draft id');
    if (draftIds.has(draftId)) fail('duplicate draft id.');
    draftIds.add(draftId);
  }
}

function textList(
  value: unknown,
  label: string,
  minEntries: number,
  maxBytes: number,
): readonly string[] {
  if (!Array.isArray(value)) fail(`${label} must be an array.`);
  if (value.length < minEntries || value.length > maxListEntries)
    fail(`${label} must list ${minEntries}..${maxListEntries} entries.`);
  return value.map((entry: unknown) => boundedText(entry, label, maxBytes));
}

function parseRefs(value: unknown, byId: ReadonlyMap<string, BoundItem>): readonly string[] {
  if (!Array.isArray(value)) fail('memory item ids must be an array.');
  if (value.length < 1 || value.length > maxRefsPerSkill)
    fail(`memory item ids must reference 1..${maxRefsPerSkill} known items.`);
  const ids = value.map((entry: unknown) => boundedId(entry, 'memory item id'));
  if (new Set(ids).size !== ids.length) fail('duplicate memory item id.');
  for (const id of ids) if (!byId.has(id)) fail(`unknown memory item id: ${id}.`);
  return ids;
}

function referencedItems(
  ids: readonly string[],
  byId: ReadonlyMap<string, BoundItem>,
): readonly BoundItem[] {
  const items: BoundItem[] = [];
  for (const id of ids) {
    const item = byId.get(id);
    if (!item) fail(`unknown memory item id: ${id}.`);
    items.push(item);
  }
  return items;
}

function spanWithin(span: SourceSpan, items: readonly BoundItem[]): boolean {
  for (const item of items) {
    for (const host of item.sourceSpans) {
      if (
        host.documentId === span.documentId &&
        host.revisionId === span.revisionId &&
        span.start >= host.start &&
        span.end <= host.end
      )
        return true;
    }
  }
  return false;
}

function parseSkillSpans(value: unknown, items: readonly BoundItem[]): readonly SourceSpan[] {
  if (!Array.isArray(value)) fail('sourceSpans must be an array.');
  if (value.length < 1 || value.length > maxSpansPerSkill)
    fail(`sourceSpans must list 1..${maxSpansPerSkill} spans.`);
  const spans = value.map(parseSpanShape);
  const seen = new Set(spans.map((span) => JSON.stringify(span)));
  if (seen.size !== spans.length) fail('duplicate source span in one skill.');
  for (const span of spans)
    if (!spanWithin(span, items))
      fail('skill span lies outside the source spans of the referenced items.');
  return spans;
}

function requireObserved(items: readonly BoundItem[]): void {
  for (const item of items) {
    if (item.kind !== 'procedure' || item.support !== 'observed')
      fail('observed skills require every referenced item to be an observed procedure.');
    if (item.contradictionIds.length > 0)
      fail('observed skills require referenced items without contradictions.');
    if (item.sourceSpans.length === 0)
      fail('observed skills require referenced items with nonempty source citations.');
  }
}

function parseSkill(raw: unknown, context: SkillContext): SkillDefinitionDraft {
  const input = fields(raw, skillFields);
  const draftId = boundedId(input.draftId, 'draft id');
  const support = choice(input.support, skillSupports, 'skill support');
  const trigger = boundedText(input.trigger, 'trigger', maxFieldBytes);
  const inputs = textList(input.inputs, 'inputs', 0, maxEntryBytes);
  const steps = textList(input.steps, 'steps', 1, maxEntryBytes);
  const outcomeCheck = boundedText(input.outcomeCheck, 'outcomeCheck', maxFieldBytes);
  const memoryItemIds = parseRefs(input.memoryItemIds, context.byId);
  const items = referencedItems(memoryItemIds, context.byId);
  const sourceSpans = parseSkillSpans(input.sourceSpans, items);
  const limitations = textList(input.limitations, 'limitations', 0, maxEntryBytes);
  if (support === 'observed') requireObserved(items);
  if (support === 'proposed' && limitations.length === 0)
    fail('proposed skills require explicit limitations.');
  return {
    draftId,
    trigger,
    inputs,
    steps,
    outcomeCheck,
    support,
    memoryItemIds,
    sourceSpans,
    limitations,
  };
}

function skillTextBytes(draft: SkillDefinitionDraft): number {
  let total = encoder.encode(draft.trigger).length + encoder.encode(draft.outcomeCheck).length;
  for (const text of [...draft.inputs, ...draft.steps, ...draft.limitations])
    total += encoder.encode(text).length;
  return total;
}

/**
 * Structural validation of untrusted model output against a host-owned scope.
 * This proves shape, bounds, snapshot consistency and cited span membership
 * only; it is not authorization, publication, or proof that a real person has
 * these capabilities. The native boundary revalidates live grants and text.
 */
export function parseMemorySkills(
  raw: unknown,
  scope: MemorySkillScope,
): readonly SkillDefinitionDraft[] {
  const bound = bindScope(scope);
  const input = fields(raw, ['skills']);
  if (!Array.isArray(input.skills)) fail('skills must be an array.');
  if (input.skills.length > scope.maxSkills)
    fail('response exceeds the host maxSkills bound; the whole result is rejected.');
  collectDraftIds(input.skills);
  const context: SkillContext = { byId: bound.byId };
  let totalTextBytes = 0;
  const drafts: SkillDefinitionDraft[] = [];
  for (const rawSkill of input.skills) {
    const draft = parseSkill(rawSkill, context);
    totalTextBytes += skillTextBytes(draft);
    if (totalTextBytes > maxTotalTextBytes)
      fail('skill text exceeds the 64 KiB total bound; the whole result is rejected.');
    drafts.push(draft);
  }
  return drafts;
}
