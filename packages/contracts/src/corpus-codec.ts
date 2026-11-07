import type { CorpusCoverage, CorpusSnapshot, RevisionReference } from './corpus';
import {
  fields,
  text,
  id,
  integer,
  flag,
  list,
  uniqueIds,
  version,
} from './corpus-wire-validation';

export function decodeCorpusCoverage(value: unknown): CorpusCoverage {
  const input = fields(value, [
    'loadedDocuments',
    'indexedDocuments',
    'analyzedDocuments',
    'usedDocuments',
    'totalDocuments',
    'loadedBytes',
    'complete',
    'limitations',
  ]);
  const result = {
    loadedDocuments: integer(input.loadedDocuments),
    indexedDocuments: integer(input.indexedDocuments),
    analyzedDocuments: integer(input.analyzedDocuments),
    usedDocuments: integer(input.usedDocuments),
    totalDocuments: input.totalDocuments === null ? null : integer(input.totalDocuments),
    loadedBytes: integer(input.loadedBytes),
    complete: flag(input.complete),
    limitations: list(input.limitations).map(text),
  };
  if (
    result.indexedDocuments > result.loadedDocuments ||
    result.analyzedDocuments > result.loadedDocuments ||
    result.usedDocuments > result.loadedDocuments ||
    (result.totalDocuments !== null &&
      (result.totalDocuments < result.loadedDocuments ||
        (result.complete && result.totalDocuments !== result.loadedDocuments)))
  )
    throw new Error('Противоречивые счётчики покрытия импорта.');
  return result;
}

function timestamp(value: unknown): string {
  const result = text(value);
  const match =
    /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2}):(\d{2})(?:\.\d{1,9})?(?:Z|[+-](\d{2}):(\d{2}))$/.exec(
      result,
    );
  if (!match) throw new Error('Неверная дата снимка корпуса.');
  const year = Number(match[1]);
  const month = Number(match[2]);
  const day = Number(match[3]);
  const hour = Number(match[4]);
  const minute = Number(match[5]);
  const second = Number(match[6]);
  const offsetHour = Number(match[7] ?? 0);
  const offsetMinute = Number(match[8] ?? 0);
  const leap = year % 4 === 0 && (year % 100 !== 0 || year % 400 === 0);
  const days = [31, leap ? 29 : 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
  if (
    month < 1 ||
    month > 12 ||
    day < 1 ||
    day > (days[month - 1] ?? 0) ||
    hour > 23 ||
    minute > 59 ||
    second > 59 ||
    offsetHour > 23 ||
    offsetMinute > 59 ||
    !Number.isFinite(Date.parse(result))
  )
    throw new Error('Неверная дата снимка корпуса.');
  return result;
}

function revisionReferences(value: unknown): RevisionReference[] {
  const result = list(value).map((item) => {
    const input = fields(item, ['documentId', 'revisionId']);
    return { documentId: id(input.documentId), revisionId: id(input.revisionId) };
  });
  const keys = result.map((item) => JSON.stringify([item.documentId, item.revisionId]));
  if (new Set(keys).size !== keys.length) throw new Error('Повторяющиеся ревизии снимка корпуса.');
  return result;
}

/** Structural boundary only; native readers must recheck current grants. */
export function decodeCorpusSnapshot(value: unknown): CorpusSnapshot {
  const input = fields(value, [
    'schemaVersion',
    'id',
    'corpusId',
    'parentSnapshotId',
    'createdAt',
    'bindingIds',
    'documentRefs',
    'episodeIds',
    'splitVersion',
    'generation',
    'coverage',
  ]);
  version(input.schemaVersion);
  return {
    schemaVersion: 1,
    id: id(input.id),
    corpusId: id(input.corpusId),
    parentSnapshotId: input.parentSnapshotId === null ? null : id(input.parentSnapshotId),
    createdAt: timestamp(input.createdAt),
    bindingIds: uniqueIds(input.bindingIds),
    documentRefs: revisionReferences(input.documentRefs),
    episodeIds: uniqueIds(input.episodeIds),
    splitVersion: id(input.splitVersion),
    generation: integer(input.generation, 1),
    coverage: decodeCorpusCoverage(input.coverage),
  };
}
