import type { OpenPersonaSessionRequest, PersonaSessionDescriptor } from './persona-memory';
import { fields } from './corpus-wire-validation';
import { memoryId } from './memory-wire-validation';
import { decodePersonaManifest } from './memory-profile-codec';
import {
  decodeMemoryPeriod,
  decodeMemorySession,
  decodeMemoryModel,
} from './research-memory-codec';

export function decodeOpenPersonaSessionRequest(value: unknown): OpenPersonaSessionRequest {
  const input = fields(value, [
    'schemaVersion',
    'operationId',
    'jobId',
    'personaId',
    'manifestId',
    'period',
  ]);
  if (input.schemaVersion !== 1) throw new Error('Неизвестная версия сессии Person.');
  return {
    schemaVersion: 1,
    operationId: memoryId(input.operationId, 128),
    jobId: memoryId(input.jobId, 128),
    personaId: memoryId(input.personaId),
    manifestId: memoryId(input.manifestId),
    period: decodeMemoryPeriod(input.period),
  };
}
export function decodePersonaSession(
  value: unknown,
  request: OpenPersonaSessionRequest,
): PersonaSessionDescriptor {
  const expected = decodeOpenPersonaSessionRequest(request);
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
    'manifest',
  ]);
  const { purpose, manifest: rawManifest, ...descriptor } = input;
  const manifest = decodePersonaManifest(rawManifest);
  if (
    purpose !== 'personaReply' ||
    manifest.id !== expected.manifestId ||
    manifest.personaId !== expected.personaId ||
    manifest.retrievalVersion !== 'nativeMemoryRetrieval:1'
  )
    throw new Error('Сессия Person не соответствует сохранённому манифесту.');
  const person = manifest.modelSettings.find((entry) => entry.component === 'person');
  if (!person) throw new Error('Модель Person не закреплена.');
  decodeMemoryModel({
    provider: person.provider,
    model: person.requestedModel,
    effort: person.requestedEffort,
  });
  const session = decodeMemorySession(descriptor, {
    personaId: expected.personaId,
    snapshotId: manifest.corpusSnapshotId,
    mode: manifest.embeddingFingerprint === null ? 'lexical' : 'hybrid',
  });
  return { ...session, purpose, manifest };
}
