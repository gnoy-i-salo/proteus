import { boundManifest, frozenManifest } from './testing/manifest-fixtures';
import { expect, it } from 'vitest';
import { decodeOpenPersonaSessionRequest, decodePersonaSession } from './persona-memory-codec';
import { personaRequest, personaSession } from './testing/persona-fixtures';

it('decodes native Person admission without accepting caller models or budgets', () => {
  const request = structuredClone(personaRequest);
  expect(decodeOpenPersonaSessionRequest(request)).toEqual(request);
  const session = personaSession();
  expect(decodePersonaSession(session, request)).toEqual(session);
  expect(session.manifest.createdAt).toBe('2026-11-09T03:00:01.004Z');
  expect(decodePersonaSession(session, request).manifest).not.toBe(session.manifest);
});
it('requires exact admission fields and bounded original IDs and periods', () => {
  for (const key of Object.keys(personaRequest)) {
    const missing: Record<string, unknown> = { ...personaRequest };
    delete missing[key];
    expect(() => decodeOpenPersonaSessionRequest(missing)).toThrow();
  }
  for (const patch of [
    { schemaVersion: 2 },
    { operationId: 'я'.repeat(65) },
    { jobId: '' },
    { personaId: 'p\n' },
    { manifestId: 'x'.repeat(257) },
    { manifestId: '\uD800' },
    { mode: 'hybrid' },
    { model: {} },
    { recallBudget: {} },
    { modelBudget: {} },
    { snapshotId: 'foreign' },
    { period: { from: 20, to: 10, includeUndated: true } },
    { period: { from: Number.MAX_SAFE_INTEGER + 1, to: null, includeUndated: true } },
    { period: { from: null, includeUndated: true } },
  ])
    expect(() => decodeOpenPersonaSessionRequest({ ...personaRequest, ...patch })).toThrow();
});
it('binds session persona, manifest, mode, configured model and strict version', () => {
  const session = personaSession();
  for (const patch of [
    { purpose: 'research' },
    { schemaVersion: 1 },
    { id: '' },
    { personaId: 'foreign' },
    { corpusSnapshotId: 'foreign' },
    { mode: 'hybrid' },
    { indexGeneration: 0 },
    { unknown: true },
    { expiresAt: 'not-a-time' },
    { manifest: { ...session.manifest, id: 'other' } },
    { manifest: { ...session.manifest, personaId: 'other' } },
    { manifest: { ...session.manifest, retrievalVersion: 'future' } },
    {
      manifest: {
        ...session.manifest,
        modelSettings: session.manifest.modelSettings.map((s) =>
          s.component === 'person' ? { ...s, requestedModel: null } : s,
        ),
      },
    },
  ])
    expect(() => decodePersonaSession({ ...session, ...patch }, personaRequest)).toThrow();
});

it('requires a bound V2 session with native full skill bodies before use', () => {
  const bundle = boundManifest();
  const value = {
    ...personaSession(),
    schemaVersion: 2,
    manifest: bundle.manifest,
    skills: bundle.skills,
  };
  const decoded = decodePersonaSession(value, personaRequest);
  expect(decoded).toEqual(value);
  expect(decoded).not.toBe(value);
  expect(() =>
    decodePersonaSession({ ...value, manifest: frozenManifest().manifest }, personaRequest),
  ).toThrow();
  expect(() => decodePersonaSession({ ...value, skills: [] }, personaRequest)).toThrow();
  expect(() =>
    decodePersonaSession({ ...value, skills: [...value.skills, value.skills[0]] }, personaRequest),
  ).toThrow();
});
it('rejects the old unbound session before any model can run', () => {
  const previous = Object.fromEntries(
    Object.entries(personaSession()).filter(([key]) => key !== 'skills'),
  );
  expect(() =>
    decodePersonaSession(
      { ...previous, schemaVersion: 1, manifest: frozenManifest().manifest },
      personaRequest,
    ),
  ).toThrow();
});
