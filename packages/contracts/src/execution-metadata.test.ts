import { describe, expect, it } from 'vitest';
import { decodeExecutionSnapshot } from './execution-metadata';
const call = {
  callId: 'call-1',
  operation: 'source-plan',
  trigger: 'initial',
  formatAttempt: 1,
  status: 'completed',
  startedAt: '2026-11-07T00:00:00.000Z',
  returnedAt: '2026-11-07T00:00:01.000Z',
  finishedAt: '2026-11-07T00:00:01.000Z',
  wallTimeMs: 1000,
  error: null,
  validationError: null,
};
const stage = {
  schemaVersion: 1,
  runId: 'run-1',
  stageId: 'stage-1',
  logicalStageId: 'sources-0',
  component: 'stalker',
  person: 0,
  label: 'Sources\nТочный текст',
  status: 'completed',
  startedAt: '2026-11-07T00:00:00.000Z',
  finishedAt: '2026-11-07T00:00:01.000Z',
  error: null,
  calls: [call],
};
const snapshot = { schemaVersion: 1, runId: 'run-1', stages: [stage] };

describe('versioned observed execution metadata', () => {
  it('keeps old traces explicitly unknown when metadata is absent', () => {
    expect(decodeExecutionSnapshot(undefined)).toBeNull();
  });
  it('roundtrips only observed lifecycle metadata without fabricating reasoning', () => {
    const decoded = decodeExecutionSnapshot(snapshot);
    expect(decoded).toEqual(snapshot);
    expect(decoded).not.toBe(snapshot);
    expect(JSON.stringify(decoded)).not.toContain('reasoning');
  });
  it.each([
    ['unknown version', { ...snapshot, schemaVersion: 2 }],
    ['mixed legacy keys', { ...snapshot, 'run-id': 'run-1' }],
    ['different run', { ...snapshot, stages: [{ ...stage, runId: 'run-2' }] }],
    ['duplicate stage', { ...snapshot, stages: [stage, stage] }],
    ['duplicate call', { ...snapshot, stages: [{ ...stage, calls: [call, call] }] }],
    ['bad time', { ...snapshot, stages: [{ ...stage, startedAt: 'yesterday' }] }],
    [
      'negative duration',
      {
        ...snapshot,
        stages: [{ ...stage, calls: [{ ...call, wallTimeMs: -1 }] }],
      },
    ],
    ['fabricated state', { ...snapshot, stages: [{ ...stage, status: 'thinking' }] }],
    ['secret field', { ...snapshot, stages: [{ ...stage, credential: 'never' }] }],
    [
      'fabricated retry',
      {
        ...snapshot,
        stages: [{ ...stage, calls: [{ ...call, trigger: 'formatRepair' }] }],
      },
    ],
    [
      'missing call identity',
      { ...snapshot, stages: [{ ...stage, calls: [{ ...call, callId: '' }] }] },
    ],
    ['null is corrupted metadata', null],
  ])('rejects %s', (_name, value) => {
    expect(() => decodeExecutionSnapshot(value)).toThrow();
  });
});
