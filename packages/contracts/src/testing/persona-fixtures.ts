import type {
  OpenPersonaSessionRequest,
  PersonaSessionDescriptor,
  PersonaContextRequest,
  PersonaContextLease,
} from '../persona-memory';
import { fabricContextRequest, fabricSession, fabricLease } from './fabric-fixtures';
import { frozenManifest } from './manifest-fixtures';

export const personaRequest: OpenPersonaSessionRequest = {
  schemaVersion: 1,
  operationId: 'persona:open',
  jobId: 'job:1',
  personaId: fabricSession.personaId,
  manifestId: frozenManifest().manifest.id,
  period: { from: null, to: null, includeUndated: true },
};
export function personaSession(): PersonaSessionDescriptor {
  return {
    ...fabricSession,
    id: 'memorySession:persona:1',
    purpose: 'personaReply',
    manifest: frozenManifest().manifest,
  };
}
export const personaContextRequest: PersonaContextRequest = {
  schemaVersion: 2,
  purpose: 'personaReply',
  manifestId: personaRequest.manifestId,
  systemPrompt: fabricContextRequest.systemPrompt,
  task: fabricContextRequest.task,
  receiptIds: [],
  memoryItemIds: [...fabricContextRequest.itemIds],
  formatAttempt: 1,
};
export function personaLease(): PersonaContextLease {
  return { ...fabricLease(), purpose: 'personaReply', manifestId: personaRequest.manifestId };
}
