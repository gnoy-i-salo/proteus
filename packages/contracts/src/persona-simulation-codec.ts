import type { AppendPersonaSimulationRequest, PersonaSimulationScope } from './persona-memory';
import type { SimulationMemoryEntry } from './memory';
import { memoryFields, memoryId, memoryText, memoryTimestamp } from './memory-wire-validation';
export function decodePersonaSimulationScope(value: unknown): PersonaSimulationScope {
  const input = memoryFields(value, ['experimentId', 'trialId']);
  return { experimentId: memoryId(input.experimentId), trialId: memoryId(input.trialId) };
}
export function decodeAppendPersonaSimulationRequest(
  value: unknown,
): AppendPersonaSimulationRequest {
  const input = memoryFields(value, [
    'schemaVersion',
    'experimentId',
    'trialId',
    'contextLeaseId',
    'invocationId',
    'answer',
  ]);
  const invocationId = memoryText(input.invocationId, 80);
  const answer = memoryText(input.answer, 4 * 1024 * 1024);
  if (input.schemaVersion !== 1 || !/^[a-zA-Z0-9-]{1,80}$/u.test(invocationId) || !answer.trim())
    throw new Error('Неверное подтверждение ответа Person.');
  return {
    schemaVersion: 1,
    experimentId: memoryId(input.experimentId),
    trialId: memoryId(input.trialId),
    contextLeaseId: memoryId(input.contextLeaseId, 128),
    invocationId,
    answer,
  };
}
export function decodePersonaSimulation(
  value: unknown,
  manifestId: string,
  request: AppendPersonaSimulationRequest,
): SimulationMemoryEntry {
  const expected = decodeAppendPersonaSimulationRequest(request);
  const input = memoryFields(value, [
    'schemaVersion',
    'id',
    'experimentId',
    'manifestId',
    'trialId',
    'role',
    'text',
    'createdAt',
  ]);
  const id = memoryId(input.id);
  const text = memoryText(input.text, 65536);
  if (
    input.schemaVersion !== 1 ||
    !/^simulation:[a-zA-Z0-9_-]+$/u.test(id) ||
    input.role !== 'simulation' ||
    input.manifestId !== manifestId ||
    input.experimentId !== expected.experimentId ||
    input.trialId !== expected.trialId ||
    !text.trim()
  )
    throw new Error('Запись симуляции не соответствует сессии Person.');
  return {
    schemaVersion: 1,
    id,
    experimentId: expected.experimentId,
    manifestId: memoryId(manifestId),
    trialId: expected.trialId,
    role: 'simulation',
    text,
    createdAt: memoryTimestamp(input.createdAt),
  };
}
