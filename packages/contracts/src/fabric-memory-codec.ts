import type {
  FabricContextRequest,
  FabricSessionDescriptor,
  OpenFabricSessionRequest,
} from './fabric-memory';
import { fields } from './corpus-wire-validation';
import { memoryId, memoryIds, memoryText } from './memory-wire-validation';
import { decodeOpenResearchSessionRequest, decodeResearchSession } from './research-memory-codec';

export function decodeOpenFabricSessionRequest(value: unknown): OpenFabricSessionRequest {
  const request = decodeOpenResearchSessionRequest(value);
  if (request.mode !== 'lexical') throw new Error('Неверный режим подготовки Fabric.');
  return { ...request, mode: 'lexical' };
}
export function decodeFabricSession(
  value: unknown,
  request: OpenFabricSessionRequest,
): FabricSessionDescriptor {
  const input = fields(value, [
    'schemaVersion',
    'id',
    'personaId',
    'corpusSnapshotId',
    'policyVersion',
    'mode',
    'indexGeneration',
    'expiresAt',
    'purpose',
  ]);
  const { purpose, ...descriptor } = input;
  if (purpose !== 'fabric') throw new Error('Неверное назначение сессии Fabric.');
  const session = decodeResearchSession(descriptor, decodeOpenFabricSessionRequest(request));
  return { ...session, mode: 'lexical', purpose };
}
export function decodeFabricContextRequest(value: unknown): FabricContextRequest {
  const input = fields(value, [
    'schemaVersion',
    'memorySnapshotId',
    'itemIds',
    'systemPrompt',
    'task',
    'formatAttempt',
  ]);
  if (input.schemaVersion !== 1 || (input.formatAttempt !== 1 && input.formatAttempt !== 2))
    throw new Error('Неверная версия или попытка подготовки Fabric.');
  return {
    schemaVersion: 1,
    memorySnapshotId: memoryId(input.memorySnapshotId),
    itemIds: memoryIds(input.itemIds),
    systemPrompt: memoryText(input.systemPrompt, 40000),
    task: memoryText(input.task, 200000),
    formatAttempt: input.formatAttempt,
  };
}
