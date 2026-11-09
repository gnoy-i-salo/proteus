import type { MemoryItem } from './memory';
import { choice, fields } from './corpus-wire-validation';
import {
  memoryId,
  memoryIds,
  memoryText,
  memoryList,
  memorySpans,
  persistedMemoryTimestamp,
} from './memory-wire-validation';

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
  const sourceSpans = memorySpans(input.sourceSpans);
  return {
    schemaVersion: 1,
    id: memoryId(input.id),
    snapshotId: memoryId(input.snapshotId),
    kind: choice(input.kind, ['episode', 'reportedAssertion', 'procedure', 'styleExample']),
    text,
    sourceSpans,
    episodeIds: memoryIds(memoryList(input.episodeIds, 64, 1)),
    eventAt: input.eventAt === null ? null : persistedMemoryTimestamp(input.eventAt),
    observedAt: persistedMemoryTimestamp(input.observedAt),
    support: choice(input.support, [
      'observed',
      'reported',
      'hypothesis',
      'unknown',
      'conflicting',
    ]),
    contradictionIds: memoryIds(memoryList(input.contradictionIds, 64)),
    limitations: memoryList(input.limitations, 16).map((value) => {
      const limitation = memoryText(value, 2048);
      if (!limitation.trim()) throw new Error('Пустое ограничение элемента памяти.');
      return limitation;
    }),
    extractionVersion: memoryId(input.extractionVersion),
    policyVersion: memoryId(input.policyVersion),
  };
}
