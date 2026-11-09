import { expect, it } from 'vitest';
import {
  decodeAppendPersonaSimulationRequest,
  decodePersonaSimulation,
} from './persona-simulation-codec';
const request = {
  schemaVersion: 1 as const,
  experimentId: 'experiment:1',
  trialId: 'trial:1',
  contextLeaseId: 'lease:1',
  invocationId: 'person-call',
  answer:
    '\uFEFF {"action":"reply","messages":[" é 😀\\r\\n "],"summary":"готово","styleConflicts":[]} \r\n',
};
const entry = {
  schemaVersion: 1 as const,
  id: 'simulation:abc123',
  experimentId: request.experimentId,
  trialId: request.trialId,
  manifestId: 'manifest:1',
  role: 'simulation' as const,
  text: ' é 😀\r\n ',
  createdAt: '2026-11-09T00:00:00.000Z',
};
it('retains exact raw output and independently checks native simulation scope', () => {
  expect(decodeAppendPersonaSimulationRequest(request)).toEqual(request);
  expect(decodePersonaSimulation(entry, entry.manifestId, request)).toEqual(entry);
});
it('rejects invented authority, legacy aliases, malformed Unicode and unbounded output', () => {
  for (const extra of ['manifestId', 'role', 'id', 'createdAt', 'text', 'experiment-id'])
    expect(() =>
      decodeAppendPersonaSimulationRequest({ ...request, [extra]: 'invented' }),
    ).toThrow();
  for (const invalid of [
    { schemaVersion: 2 },
    { invocationId: 'call:1' },
    { invocationId: 'a'.repeat(81) },
    { contextLeaseId: '' },
    { trialId: 'x\n' },
    { experimentId: 'x'.repeat(257) },
    { answer: ' ' },
    { answer: '\uD800' },
    { answer: 'x'.repeat(4194305) },
  ])
    expect(() => decodeAppendPersonaSimulationRequest({ ...request, ...invalid })).toThrow();
});
it('rejects foreign/native-overridden identity, corrupt receipts and invalid stored text', () => {
  for (const invalid of [
    { schemaVersion: 2 },
    { id: 'source:1' },
    { role: 'user' },
    { manifestId: 'foreign' },
    { experimentId: 'foreign' },
    { trialId: 'foreign' },
    { createdAt: '2026-02-30T00:00:00.000Z' },
    { text: ' ' },
    { text: 'x'.repeat(65537) },
    { text: '\uD800' },
    JSON.parse('{"trial-id":"trial:1"}') as Record<string, unknown>,
  ])
    expect(() =>
      decodePersonaSimulation({ ...entry, ...invalid }, entry.manifestId, request),
    ).toThrow();
  expect(
    decodePersonaSimulation({ ...entry, text: 'x'.repeat(65536) }, entry.manifestId, request).text,
  ).toHaveLength(65536);
});
