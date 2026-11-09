import type { OpenResearchSessionRequest, ResearchSessionDescriptor } from './research-memory';
import type { ModelCallBudget, RecallBudget } from './memory';
import { choice, fields, flag, integer } from './corpus-wire-validation';
import { memoryId, memoryText, memoryTimestamp } from './memory-wire-validation';

function bounded(value: unknown, maximum: number): number {
  const result = integer(value, 1);
  if (result > maximum) throw new Error('Превышен бюджет памяти.');
  return result;
}
export function decodeRecallBudget(value: unknown): RecallBudget {
  const input = fields(value, [
    'maxCalls',
    'maxCandidates',
    'maxEpisodes',
    'maxSourceBytes',
    'maxContextTokens',
    'maxDurationMs',
  ]);
  return {
    maxCalls: bounded(input.maxCalls, 1000),
    maxCandidates: bounded(input.maxCandidates, 100000),
    maxEpisodes: bounded(input.maxEpisodes, 1000),
    maxSourceBytes: bounded(input.maxSourceBytes, 64 * 1024 * 1024),
    maxContextTokens: bounded(input.maxContextTokens, 1000000),
    maxDurationMs: bounded(input.maxDurationMs, 3600000),
  };
}
export function decodeModelCallBudget(value: unknown): ModelCallBudget {
  const input = fields(value, ['maxCalls', 'maxInputBytes', 'maxDurationMs']);
  return {
    maxCalls: bounded(input.maxCalls, 1000),
    maxInputBytes: bounded(input.maxInputBytes, 64 * 1024 * 1024),
    maxDurationMs: bounded(input.maxDurationMs, 3600000),
  };
}
export function decodeMemoryPeriod(value: unknown): OpenResearchSessionRequest['period'] {
  const input = fields(value, ['from', 'to', 'includeUndated']);
  const from = input.from === null ? null : integer(input.from, Number.MIN_SAFE_INTEGER);
  const to = input.to === null ? null : integer(input.to, Number.MIN_SAFE_INTEGER);
  if (from !== null && to !== null && from > to) throw new Error('Неверный период исследования.');
  return { from, to, includeUndated: flag(input.includeUndated) };
}
export function decodeMemoryModel(value: unknown): OpenResearchSessionRequest['model'] {
  const input = fields(value, ['provider', 'model', 'effort']);
  const selected = memoryText(input.model, 120);
  if (!selected.trim() || /[\r\n\0]/u.test(selected))
    throw new Error('Неверная модель исследования.');
  return {
    provider: choice(input.provider, ['codex', 'kimi']),
    model: selected,
    effort: choice(input.effort, ['low', 'medium', 'high']),
  };
}

export function decodeOpenResearchSessionRequest(value: unknown): OpenResearchSessionRequest {
  const input = fields(value, [
    'schemaVersion',
    'operationId',
    'jobId',
    'snapshotId',
    'personaId',
    'mode',
    'period',
    'recallBudget',
    'modelBudget',
    'model',
  ]);
  if (input.schemaVersion !== 1) throw new Error('Неизвестная версия запроса исследования.');
  return {
    schemaVersion: 1,
    operationId: memoryId(input.operationId, 128),
    jobId: memoryId(input.jobId, 128),
    snapshotId: memoryId(input.snapshotId),
    personaId: memoryId(input.personaId),
    mode: choice(input.mode, ['lexical', 'hybrid']),
    period: decodeMemoryPeriod(input.period),
    recallBudget: decodeRecallBudget(input.recallBudget),
    modelBudget: decodeModelCallBudget(input.modelBudget),
    model: decodeMemoryModel(input.model),
  };
}
export function decodeResearchSession(
  value: unknown,
  request: OpenResearchSessionRequest,
): ResearchSessionDescriptor {
  return decodeMemorySession(value, decodeOpenResearchSessionRequest(request));
}
export function decodeMemorySession(
  value: unknown,
  expected: Pick<OpenResearchSessionRequest, 'personaId' | 'snapshotId' | 'mode'>,
): ResearchSessionDescriptor {
  const input = fields(value, [
    'schemaVersion',
    'id',
    'personaId',
    'corpusSnapshotId',
    'policyVersion',
    'mode',
    'indexGeneration',
    'expiresAt',
  ]);
  if (
    input.schemaVersion !== 1 ||
    input.personaId !== expected.personaId ||
    input.corpusSnapshotId !== expected.snapshotId ||
    input.mode !== expected.mode
  )
    throw new Error('Сессия не соответствует запросу исследования.');
  const expiresAt = memoryTimestamp(input.expiresAt, 'Неверный срок исследовательской сессии.');
  return {
    schemaVersion: 1,
    id: memoryId(input.id, 128),
    personaId: expected.personaId,
    corpusSnapshotId: expected.snapshotId,
    policyVersion: memoryId(input.policyVersion),
    mode: expected.mode,
    indexGeneration: integer(input.indexGeneration, 1),
    expiresAt,
  };
}
