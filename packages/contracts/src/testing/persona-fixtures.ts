import type {
  OpenPersonaSessionRequest,
  PersonaSessionDescriptor,
  PersonaContextRequest,
  PersonaContextLease,
} from '../persona-memory';
import { fabricContextRequest, fabricSession, fabricLease } from './fabric-fixtures';
import { frozenManifest, boundManifest } from './manifest-fixtures';

export const personaRequest: OpenPersonaSessionRequest = {
  schemaVersion: 1,
  operationId: 'persona:open',
  jobId: 'job:1',
  personaId: fabricSession.personaId,
  manifestId: frozenManifest().manifest.id,
  period: { from: null, to: null, includeUndated: true },
};
export function personaSession(): PersonaSessionDescriptor {
  const bundle = boundManifest();
  if (bundle.manifest.schemaVersion !== 2) throw new Error('Fixture must be bound.');
  return {
    ...fabricSession,
    schemaVersion: 2,
    skills: bundle.skills,
    id: 'memorySession:persona:1',
    purpose: 'personaReply',
    manifest: bundle.manifest,
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
