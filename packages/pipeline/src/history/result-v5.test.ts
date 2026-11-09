import { expect, it } from 'vitest';
import { readStoredResult } from './read-stored-result';
import { prepareStoredResult } from './prepare-stored-result';
import fixture from '../testing/legacy-results/v4-current-with-execution.json';

function version5() {
  const value = structuredClone(fixture);
  return {
    ...value,
    schemaVersion: 5,
    people: value.people.map((person, index) => ({
      ...person,
      participantId: index === 0 ? 'a' : 'b',
      profile: { ...person.profile, id: 'saved-person-Ёж' },
      baseProfile: { ...person.baseProfile, id: 'saved-person-Ёж' },
    })),
    control: {
      ...value.control,
      participantId: 'neutral',
      profile: { ...value.control.profile, id: 'saved-neutral' },
      baseProfile: { ...value.control.baseProfile, id: 'saved-neutral' },
    },
  };
}

it('reads explicit roles while preserving stable identity shared by A and B', () => {
  const value = version5();
  const text = JSON.stringify(value);
  expect(readStoredResult(text)).toEqual({ sourceVersion: 5, result: value });
  expect(JSON.stringify(value)).toBe(text);
});

it.each(['participantId', 'profile', 'score', 'trials', 'control', 'aliases'])(
  'rejects conflicting v5 %s bindings without changing historical observations',
  (field) => {
    const value = version5();
    if (field === 'participantId') value.people[1]!.participantId = 'a';
    if (field === 'profile') value.people[0]!.baseProfile.id = 'foreign-person';
    if (field === 'score') value.people[0]!.score.personId = 'b';
    if (field === 'trials') value.people[0]!.trials = value.people[1]!.trials;
    if (field === 'control') value.control.participantId = 'a';
    if (field === 'aliases') {
      const legacyAlias: unknown = JSON.parse('{"participant-id":"a"}');
      Object.assign(value.people[0]!, legacyAlias);
    }
    const text = JSON.stringify(value);
    expect(() => readStoredResult(text)).toThrow();
    expect(JSON.stringify(value)).toBe(text);
  },
);

it('keeps version4 strict and rejects new role fields under the old schema', () => {
  const value = structuredClone(fixture);
  Object.assign(value.people[0]!, { participantId: 'a' });
  expect(() => readStoredResult(JSON.stringify(value))).toThrow();
});

function memoryVersion5() {
  const value = version5();
  const participants = [...value.people, value.control].map((person) => {
    const memory = {
      schemaVersion: 1,
      manifestId: `manifest-${person.participantId}`,
      memorySnapshotId: `snapshot-${person.participantId}`,
      skills: [],
    };
    const base = { ...person.baseProfile, memory, taskSkills: [], skills: [] };
    return {
      ...person,
      baseProfile: base,
      profile: structuredClone(base),
      taskAdaptation: {
        schemaVersion: 1,
        manifestId: memory.manifestId,
        task: value.input.task,
        strategies: [
          {
            id: 's1',
            instruction: '  Стратегия 👩🏽‍🔬\r\n',
            status: 'hypothesis',
            evidenceIds: [],
            limitations: ['Общая стратегия'],
          },
        ],
      },
    };
  });
  return {
    ...value,
    people: participants.slice(0, 2),
    control: participants[2]!,
    protocol: {
      ...value.protocol,
      memory: {
        schemaVersion: 1,
        mode: 'fixedContext',
        participants: participants.map((person) => ({
          participantId: person.participantId,
          manifestId: person.profile.memory.manifestId,
        })),
        neutralKnowledgeCondition: 'unmatched',
        limitations: ['Сопоставимость не установлена'],
      },
    },
  };
}

it('preserves separate task guidance, frozen profiles and explicit memory bindings', () => {
  const value = memoryVersion5();
  expect(readStoredResult(JSON.stringify(value))).toEqual({ sourceVersion: 5, result: value });
});

it.each(['task', 'manifest', 'missingProtocol', 'changedBase', 'promotedClaim'])(
  'rejects invalid memory binding: %s',
  (field) => {
    const value = memoryVersion5();
    if (field === 'task') value.people[0]!.taskAdaptation.task += ' changed';
    if (field === 'manifest') value.protocol.memory.participants[0]!.manifestId = 'foreign';
    if (field === 'missingProtocol') Reflect.deleteProperty(value.protocol, 'memory');
    if (field === 'changedBase') value.people[0]!.profile.archetype += ' task strategy';
    if (field === 'promotedClaim')
      value.people[0]!.taskAdaptation.strategies[0]!.status = 'supported';
    expect(() => readStoredResult(JSON.stringify(value))).toThrow();
  },
);

it('binds version5 protocol to the exact task recorded for the series', () => {
  const value = version5();
  value.protocol.task += '\r\nother';
  expect(() => readStoredResult(JSON.stringify(value))).toThrow();
});

it('rejects duplicated role-to-manifest bindings instead of selecting the first', () => {
  const value = memoryVersion5();
  value.protocol.memory.participants.push({
    ...value.protocol.memory.participants[0]!,
    manifestId: 'foreign',
  });
  expect(() => readStoredResult(JSON.stringify(value))).toThrow();
});

it('applies the existing export policy to v5 without changing roles or task guidance', () => {
  const value = memoryVersion5();
  const stored = readStoredResult(JSON.stringify(value));
  const before = JSON.stringify(stored);
  const prepared = prepareStoredResult(stored);
  expect(prepared.view.sourceVersion).toBe(5);
  if (prepared.view.sourceVersion !== 5) throw new Error('Expected version5');
  expect(prepared.view.result.people.map((person) => person.participantId)).toEqual(['a', 'b']);
  expect(prepared.view.result.people[0]!.taskAdaptation).toEqual(value.people[0]!.taskAdaptation);
  expect(prepared.view.result.exportMetadata).toBeDefined();
  expect(JSON.stringify(stored)).toBe(before);
});
