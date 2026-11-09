import type { ExecutionSnapshot, ModelCallLifecycle, StageLifecycle } from './execution';

function invalid(): never {
  throw new Error('Invalid execution metadata');
}
function fields(value: unknown, keys: readonly string[]): Record<string, unknown> {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return invalid();
  const record = value as Record<string, unknown>;
  if (
    Object.keys(record).length !== keys.length ||
    Object.keys(record).some((key) => !keys.includes(key))
  )
    return invalid();
  return record;
}
function text(value: unknown): string {
  if (typeof value !== 'string' || value.length > 24000) return invalid();
  return value;
}
function identifier(value: unknown): string {
  const result = text(value);
  return /^[A-Za-z0-9_-]{1,128}$/u.test(result) ? result : invalid();
}
function nullableText(value: unknown): string | null {
  return value === null ? null : text(value);
}
function timestamp(value: unknown): string | null {
  if (value === null) return null;
  const result = text(value);
  if (
    !/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z$/u.test(result) ||
    !Number.isFinite(Date.parse(result)) ||
    new Date(result).toISOString() !== result
  )
    return invalid();
  return result;
}
function choice<T extends string>(value: unknown, allowed: readonly T[]): T {
  return typeof value === 'string' && allowed.includes(value as T) ? (value as T) : invalid();
}
function call(value: unknown, ids: Set<string>): ModelCallLifecycle {
  const input = fields(value, [
    'callId',
    'operation',
    'trigger',
    'formatAttempt',
    'status',
    'startedAt',
    'returnedAt',
    'finishedAt',
    'wallTimeMs',
    'error',
    'validationError',
  ]);
  const callId = identifier(input['callId']);
  if (ids.has(callId)) return invalid();
  ids.add(callId);
  const formatAttempt = input['formatAttempt'];
  const trigger = choice(input['trigger'], ['initial', 'formatRepair']);
  if (
    (formatAttempt !== 1 && formatAttempt !== 2) ||
    (trigger === 'formatRepair') !== (formatAttempt === 2)
  )
    return invalid();
  const wallTimeMs = input['wallTimeMs'];
  if (
    wallTimeMs !== null &&
    (typeof wallTimeMs !== 'number' || !Number.isFinite(wallTimeMs) || wallTimeMs < 0)
  )
    return invalid();
  const startedAt = timestamp(input['startedAt']);
  if (startedAt === null) return invalid();
  const status = choice(input['status'], [
    'running',
    'validating',
    'completed',
    'failed',
    'cancelled',
  ]);
  const finishedAt = timestamp(input['finishedAt']);
  if (['completed', 'failed', 'cancelled'].includes(status) !== (finishedAt !== null))
    return invalid();
  return {
    callId,
    operation: choice(input['operation'], [
      'generation',
      'public-research',
      'source-plan',
      'corpus-analysis',
      'persona-reply',
    ]),
    trigger,
    formatAttempt,
    status,
    startedAt,
    returnedAt: timestamp(input['returnedAt']),
    finishedAt,
    wallTimeMs,
    error: nullableText(input['error']),
    validationError: nullableText(input['validationError']),
  };
}
function stage(
  value: unknown,
  runId: string,
  stageIds: Set<string>,
  callIds: Set<string>,
): StageLifecycle {
  const input = fields(value, [
    'schemaVersion',
    'runId',
    'stageId',
    'logicalStageId',
    'component',
    'person',
    'label',
    'status',
    'startedAt',
    'finishedAt',
    'error',
    'calls',
  ]);
  if (input['schemaVersion'] !== 1 || input['runId'] !== runId) return invalid();
  const stageId = identifier(input['stageId']);
  if (stageIds.has(stageId)) return invalid();
  stageIds.add(stageId);
  const person = input['person'];
  if (
    person !== null &&
    (typeof person !== 'number' || !Number.isInteger(person) || person < 0 || person > 2)
  )
    return invalid();
  const rawCalls = input['calls'];
  if (!Array.isArray(rawCalls) || rawCalls.length > 256) return invalid();
  const status = choice(input['status'], [
    'running',
    'validating',
    'formatRetry',
    'completed',
    'failed',
    'cancelled',
    'skipped',
  ]);
  const startedAt = timestamp(input['startedAt']);
  const finishedAt = timestamp(input['finishedAt']);
  if (
    (status === 'skipped') !== (startedAt === null) ||
    ['completed', 'failed', 'cancelled', 'skipped'].includes(status) !== (finishedAt !== null)
  )
    return invalid();
  if (status === 'skipped' && rawCalls.length) return invalid();
  return {
    schemaVersion: 1,
    runId,
    stageId,
    logicalStageId: identifier(input['logicalStageId']),
    component: choice(input['component'], [
      'stalker',
      'person-fabric',
      'person',
      'brain-analyzator',
      'brain-scoring',
    ]),
    person,
    label: text(input['label']),
    status,
    startedAt,
    finishedAt,
    error: nullableText(input['error']),
    calls: rawCalls.map((value) => call(value, callIds)),
  };
}

/** Missing extension means unknown legacy metadata, never inferred call/retry counts. */
export function decodeExecutionSnapshot(value: unknown): ExecutionSnapshot | null {
  if (value === undefined) return null;
  const input = fields(value, ['schemaVersion', 'runId', 'stages']);
  if (input['schemaVersion'] !== 1) return invalid();
  const runId = identifier(input['runId']);
  const rawStages = input['stages'];
  if (!Array.isArray(rawStages) || rawStages.length > 512) return invalid();
  const stageIds = new Set<string>();
  const callIds = new Set<string>();
  return {
    schemaVersion: 1,
    runId,
    stages: rawStages.map((value) => stage(value, runId, stageIds, callIds)),
  };
}
