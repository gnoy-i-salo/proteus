import { describe, expect, it } from 'vitest';
import { decodeOpenResearchSessionRequest, decodeResearchSession } from './research-memory-codec';
import type { OpenResearchSessionRequest } from './research-memory';

const request = (): OpenResearchSessionRequest => ({
  schemaVersion: 1,
  operationId: 'research:1',
  jobId: 'job:1',
  snapshotId: 'snapshot:1',
  personaId: 'Анна',
  mode: 'lexical',
  period: { from: null, to: null, includeUndated: true },
  recallBudget: {
    maxCalls: 3,
    maxCandidates: 100,
    maxEpisodes: 10,
    maxSourceBytes: 1048576,
    maxContextTokens: 10000,
    maxDurationMs: 60000,
  },
  modelBudget: { maxCalls: 2, maxInputBytes: 100000, maxDurationMs: 1000 },
  model: { provider: 'codex', model: 'fixture-model', effort: 'medium' },
});
const descriptor = () => ({
  schemaVersion: 1,
  id: 'session:1',
  personaId: 'Анна',
  corpusSnapshotId: 'snapshot:1',
  policyVersion: 'policy:1',
  mode: 'lexical',
  indexGeneration: 2,
  expiresAt: '2026-11-09T03:01:00.000Z',
});
describe('research session boundary', () => {
  it('preserves explicit nullable period, IDs and selected model', () => {
    const input = request();
    expect(decodeOpenResearchSessionRequest(input)).toEqual(input);
    expect(decodeResearchSession(descriptor(), input)).toEqual(descriptor());
  });
  it.each([
    { schemaVersion: 2 },
    { grantId: 'forged' },
    { operationId: 'a'.repeat(129) },
    { jobId: ' ' },
    { snapshotId: 'x\n' },
    { personaId: 'я'.repeat(129) },
    { mode: 'auto' },
    { period: { from: 2, to: 1, includeUndated: false } },
    { period: { from: 0, includeUndated: true } },
    { period: { from: Number.MIN_SAFE_INTEGER - 1, to: null, includeUndated: true } },
    { model: { provider: 'cloud', model: 'm', effort: 'medium' } },
    { model: { provider: 'codex', model: 'm\n', effort: 'medium' } },
    { model: { provider: 'codex', model: 'm', effort: 'medium', operation: 'persona-reply' } },
  ])('rejects invalid or expanded admission: %j', (patch) => {
    const input = { ...request(), ...patch };
    const before = structuredClone(input);
    expect(() => decodeOpenResearchSessionRequest(input)).toThrow();
    expect(input).toEqual(before);
  });
  it('enforces every native budget bound before invoking IPC', () => {
    const caps = {
      maxCalls: 1000,
      maxCandidates: 100000,
      maxEpisodes: 1000,
      maxSourceBytes: 64 * 1024 * 1024,
      maxContextTokens: 1000000,
      maxDurationMs: 3600000,
    };
    for (const [key, cap] of Object.entries(caps)) {
      expect(() =>
        decodeOpenResearchSessionRequest({
          ...request(),
          recallBudget: { ...request().recallBudget, [key]: cap },
        }),
      ).not.toThrow();
      for (const invalid of [0, cap + 1, 1.5, Infinity])
        expect(() =>
          decodeOpenResearchSessionRequest({
            ...request(),
            recallBudget: { ...request().recallBudget, [key]: invalid },
          }),
        ).toThrow();
    }
    for (const [key, cap] of Object.entries({
      maxCalls: 1000,
      maxInputBytes: 64 * 1024 * 1024,
      maxDurationMs: 3600000,
    })) {
      for (const invalid of [0, cap + 1, 1.5, Infinity])
        expect(() =>
          decodeOpenResearchSessionRequest({
            ...request(),
            modelBudget: { ...request().modelBudget, [key]: invalid },
          }),
        ).toThrow();
    }
  });
  it.each([
    { schemaVersion: 2 },
    { id: '' },
    { personaId: 'Борис' },
    { corpusSnapshotId: 'foreign' },
    { mode: 'hybrid' },
    { policyVersion: '' },
    { indexGeneration: 0 },
    { expiresAt: '2026-02-30T00:00:00.000Z' },
    { expiresAt: 'not-a-date' },
    { hidden: true },
  ])('rejects mismatched session descriptors: %j', (patch) => {
    expect(() => decodeResearchSession({ ...descriptor(), ...patch }, request())).toThrow();
  });
  it('requires all request and descriptor fields', () => {
    for (const key of Object.keys(request())) {
      const value: Record<string, unknown> = { ...request() };
      delete value[key];
      expect(() => decodeOpenResearchSessionRequest(value)).toThrow();
    }
    for (const key of Object.keys(descriptor())) {
      const value: Record<string, unknown> = { ...descriptor() };
      delete value[key];
      expect(() => decodeResearchSession(value, request())).toThrow();
    }
  });
});
