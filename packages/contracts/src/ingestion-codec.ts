import type {
  IngestionBudget,
  IngestionJob,
  IngestionPreview,
  IngestionSelection,
} from './ingestion';
import type { CorpusCoverage } from './corpus';
import { decodeSourceBinding } from './source-binding-codec';

function fields(value: unknown, keys: readonly string[]): Record<string, unknown> {
  if (
    !value ||
    typeof value !== 'object' ||
    Array.isArray(value) ||
    Object.keys(value).length !== keys.length ||
    Object.keys(value).some((key) => !keys.includes(key))
  )
    throw new Error('Неверная схема импорта.');
  return value as Record<string, unknown>;
}
function text(value: unknown): string {
  if (typeof value !== 'string') throw new Error('Неверное текстовое поле импорта.');
  return value;
}
function id(value: unknown): string {
  const result = text(value);
  if (!result.trim()) throw new Error('Отсутствует идентификатор импорта.');
  return result;
}
function nullableText(value: unknown): string | null {
  return value === null ? null : text(value);
}
function integer(value: unknown, minimum = 0): number {
  if (typeof value !== 'number' || !Number.isSafeInteger(value) || value < minimum)
    throw new Error('Неверный счётчик импорта.');
  return value;
}
function flag(value: unknown): boolean {
  if (typeof value !== 'boolean') throw new Error('Неверный флаг импорта.');
  return value;
}
function list(value: unknown): unknown[] {
  if (!Array.isArray(value)) throw new Error('Неверный список импорта.');
  return value as unknown[];
}
function uniqueIds(value: unknown): string[] {
  const result = list(value).map(id);
  if (new Set(result).size !== result.length)
    throw new Error('Повторяющиеся идентификаторы импорта.');
  return result;
}
function choice<T extends string>(value: unknown, allowed: readonly T[]): T {
  const result = allowed.find((item) => item === value);
  if (result === undefined) throw new Error('Неизвестное состояние импорта.');
  return result;
}
function version(value: unknown): void {
  if (value !== 1) throw new Error('Неизвестная версия импорта.');
}

export function decodeIngestionBudget(value: unknown): IngestionBudget {
  const input = fields(value, [
    'maxInputBytes',
    'maxExpandedBytes',
    'maxDocuments',
    'maxBatchDocuments',
    'maxBatchBytes',
    'maxDurationMs',
  ]);
  const budget = {
    maxInputBytes: integer(input.maxInputBytes, 1),
    maxExpandedBytes: integer(input.maxExpandedBytes, 1),
    maxDocuments: integer(input.maxDocuments, 1),
    maxBatchDocuments: integer(input.maxBatchDocuments, 1),
    maxBatchBytes: integer(input.maxBatchBytes, 1),
    maxDurationMs: integer(input.maxDurationMs, 1),
  };
  if (
    budget.maxBatchDocuments > budget.maxDocuments ||
    budget.maxBatchBytes > budget.maxExpandedBytes
  )
    throw new Error('Размер порции превышает лимит импорта.');
  return budget;
}

export function decodeIngestionSelection(value: unknown): IngestionSelection {
  const input = fields(value, ['previewId', 'resourceIds', 'bindings']);
  const resourceIds = uniqueIds(input.resourceIds);
  const bindings = list(input.bindings).map(decodeSourceBinding);
  if (
    !resourceIds.length ||
    !bindings.length ||
    new Set(bindings.map((binding) => binding.id)).size !== bindings.length ||
    bindings.some((binding) => !resourceIds.includes(binding.resourceId)) ||
    resourceIds.some((resourceId) => !bindings.some((binding) => binding.resourceId === resourceId))
  )
    throw new Error('Привязки не соответствуют выбранным источникам импорта.');
  return { previewId: id(input.previewId), resourceIds, bindings };
}

export function decodeIngestionPreview(value: unknown): IngestionPreview {
  const input = fields(value, [
    'schemaVersion',
    'id',
    'sourceHandle',
    'sourceFormat',
    'adapterVersion',
    'resources',
    'limitations',
  ]);
  version(input.schemaVersion);
  const resources = list(input.resources).map((value) => {
    const resource = fields(value, ['id', 'label', 'authorIds', 'estimatedDocuments']);
    return {
      id: id(resource.id),
      label: text(resource.label),
      authorIds: uniqueIds(resource.authorIds),
      estimatedDocuments:
        resource.estimatedDocuments === null ? null : integer(resource.estimatedDocuments),
    };
  });
  uniqueIds(resources.map((resource) => resource.id));
  return {
    schemaVersion: 1,
    id: id(input.id),
    sourceHandle: id(input.sourceHandle),
    sourceFormat: id(input.sourceFormat),
    adapterVersion: id(input.adapterVersion),
    resources,
    limitations: list(input.limitations).map(text),
  };
}

function coverage(value: unknown): CorpusCoverage {
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
function checkpoint(value: unknown): IngestionJob['checkpoint'] {
  const input = fields(value, ['adapterVersion', 'cursor', 'committedBatch', 'stagingGeneration']);
  return {
    adapterVersion: id(input.adapterVersion),
    cursor: nullableText(input.cursor),
    committedBatch: integer(input.committedBatch),
    stagingGeneration: integer(input.stagingGeneration),
  };
}
function failure(value: unknown): IngestionJob['error'] {
  if (value === null) return null;
  const input = fields(value, ['code', 'message', 'retryable']);
  return { code: id(input.code), message: text(input.message), retryable: flag(input.retryable) };
}

/** Structural boundary only. Native must authorize every operation and publication. */
export function decodeIngestionJob(value: unknown): IngestionJob {
  const input = fields(value, [
    'schemaVersion',
    'id',
    'selection',
    'status',
    'phase',
    'budget',
    'checkpoint',
    'coverage',
    'publishedSnapshotId',
    'error',
  ]);
  version(input.schemaVersion);
  const job: IngestionJob = {
    schemaVersion: 1,
    id: id(input.id),
    selection: decodeIngestionSelection(input.selection),
    status: choice(input.status, [
      'queued',
      'running',
      'paused',
      'cancelled',
      'failed',
      'completed',
    ]),
    phase: choice(input.phase, ['reading', 'normalizing', 'episodes', 'indexing', 'publishing']),
    budget: decodeIngestionBudget(input.budget),
    checkpoint: checkpoint(input.checkpoint),
    coverage: coverage(input.coverage),
    publishedSnapshotId: input.publishedSnapshotId === null ? null : id(input.publishedSnapshotId),
    error: failure(input.error),
  };
  if (
    (job.status === 'completed' && (!job.coverage.complete || job.error !== null)) ||
    (job.status === 'failed' && job.error === null) ||
    (job.status !== 'completed' && job.publishedSnapshotId !== null)
  )
    throw new Error('Противоречивый результат импорта.');
  if (
    job.coverage.loadedDocuments > job.budget.maxDocuments ||
    job.coverage.loadedBytes > job.budget.maxExpandedBytes
  )
    throw new Error('Импорт превысил выделенный бюджет.');
  return job;
}
