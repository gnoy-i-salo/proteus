import type { MemoryItem } from './memory';
import { choice, fields, list } from './corpus-wire-validation';
import { memoryId, memoryIds, memoryText } from './memory-wire-validation';
import { decodeSourceSpan } from './source-excerpt-codec';

// Persisted native dates accept RFC3339 offsets and fractional seconds. Session
// deadlines use a different, canonical UTC wire format; do not normalize stored dates.
function timestamp(value: unknown): string {
  const result = memoryText(value, 64);
  const parts =
    /^(\d{4})-(\d{2})-(\d{2})[Tt ](\d{2}):(\d{2}):(\d{2})(?:\.\d+)?(?:[Zz]|([+-])(\d{2}):(\d{2}))$/u.exec(
      result,
    );
  if (!parts) throw new Error('Неверная дата элемента памяти.');
  const year = Number(parts[1]);
  const month = Number(parts[2]);
  const day = Number(parts[3]);
  const leap = year % 4 === 0 && (year % 100 !== 0 || year % 400 === 0);
  const days = [31, leap ? 29 : 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
  if (
    month < 1 ||
    month > 12 ||
    day < 1 ||
    day > days[month - 1]! ||
    Number(parts[4]) > 23 ||
    Number(parts[5]) > 59 ||
    Number(parts[6]) > 60 ||
    (parts[7] !== undefined && (Number(parts[8]) > 23 || Number(parts[9]) > 59))
  )
    throw new Error('Неверная дата элемента памяти.');
  return result;
}

function boundedList(value: unknown, maximum: number, minimum = 0): unknown[] {
  const values = list(value);
  if (values.length < minimum || values.length > maximum)
    throw new Error('Неверное число ссылок элемента памяти.');
  return values;
}

/** Structural reader; native publication/dispatch owns source truth and live grants. */
export function decodeMemoryItem(value: unknown): MemoryItem {
  const input = fields(value, [
    'schemaVersion',
    'id',
    'snapshotId',
    'kind',
    'text',
    'sourceSpans',
    'episodeIds',
    'eventAt',
    'observedAt',
    'support',
    'contradictionIds',
    'limitations',
    'extractionVersion',
    'policyVersion',
  ]);
  if (input.schemaVersion !== 1) throw new Error('Неизвестная версия элемента памяти.');
  const text = memoryText(input.text, 64 * 1024);
  if (!text.length) throw new Error('Пустой текст элемента памяти.');
  const sourceSpans = boundedList(input.sourceSpans, 64, 1).map((value) => {
    const span = decodeSourceSpan(value);
    memoryId(span.documentId);
    memoryId(span.revisionId);
    return span;
  });
  if (new Set(sourceSpans.map((span) => JSON.stringify(span))).size !== sourceSpans.length)
    throw new Error('Повторяющийся интервал элемента памяти.');
  return {
    schemaVersion: 1,
    id: memoryId(input.id),
    snapshotId: memoryId(input.snapshotId),
    kind: choice(input.kind, ['episode', 'reportedAssertion', 'procedure', 'styleExample']),
    text,
    sourceSpans,
    episodeIds: memoryIds(boundedList(input.episodeIds, 64, 1)),
    eventAt: input.eventAt === null ? null : timestamp(input.eventAt),
    observedAt: timestamp(input.observedAt),
    support: choice(input.support, [
      'observed',
      'reported',
      'hypothesis',
      'unknown',
      'conflicting',
    ]),
    contradictionIds: memoryIds(boundedList(input.contradictionIds, 64)),
    limitations: boundedList(input.limitations, 16).map((value) => {
      const limitation = memoryText(value, 2048);
      if (!limitation.trim()) throw new Error('Пустое ограничение элемента памяти.');
      return limitation;
    }),
    extractionVersion: memoryId(input.extractionVersion),
    policyVersion: memoryId(input.policyVersion),
  };
}
