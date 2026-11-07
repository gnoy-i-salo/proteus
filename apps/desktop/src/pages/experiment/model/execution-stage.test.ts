import { describe, expect, it } from 'vitest';
import type { StageLifecycle } from '@proteus-int/contracts';
import { classifyExecutionStage, classifyTraceStage } from './execution-stage';
const knownStages = [
  ['rubric', 'brain-analyzator', null, 'system', null, 'protocol', 'Brain Analyzator'],
  ['sources-0', 'stalker', 0, 'a', null, 'research', 'STALKER'],
  ['sources-1', 'stalker', 1, 'b', null, 'research', 'STALKER'],
  ['profile-0', 'person-fabric', 0, 'a', null, 'profile', 'Person Fabric'],
  ['profile-1', 'person-fabric', 1, 'b', null, 'profile', 'Person Fabric'],
  ['adapt-0', 'person-fabric', 0, 'a', null, 'adaptation', 'Person Fabric'],
  ['adapt-1', 'person-fabric', 1, 'b', null, 'adaptation', 'Person Fabric'],
  ['adapt-2', 'person-fabric', 2, 'control', null, 'adaptation', 'Person Fabric'],
] as const;
describe('exact execution identities', () => {
  it.each(knownStages)(
    '%s identifies the declared preparation phase',
    (logicalStageId, component, person, participant, round, phase, componentLabel) => {
      expect(
        classifyExecutionStage({
          logicalStageId,
          component,
          person,
          label: 'Exact label 👩🏽‍🔬',
        }),
      ).toEqual({ participant, round, phase, componentLabel });
    },
  );
  for (const round of [1, 2, 3]) {
    for (const [id, person, participant] of [
      ['a', 0, 'a'],
      ['b', 1, 'b'],
      ['neutral', 2, 'control'],
    ] as const) {
      it(`identifies solution ${round}-${id}`, () => {
        expect(
          classifyExecutionStage({
            logicalStageId: `solve-${round}-${id}`,
            component: 'person',
            person,
            label: 'Not parsed',
          }),
        ).toEqual({
          participant,
          round,
          phase: 'solution',
          componentLabel: 'Person',
        });
      });
    }
    for (const pass of [0, 1]) {
      it(`identifies evaluation ${round}-${pass} as shared`, () => {
        expect(
          classifyExecutionStage({
            logicalStageId: `judge-${round}-${pass}`,
            component: 'brain-analyzator',
            person: null,
            label: 'Not parsed',
          }),
        ).toEqual({
          participant: 'system',
          round,
          phase: 'evaluation',
          componentLabel: 'Brain Analyzator',
        });
      });
    }
  }
});
it.each([
  ['solve-0-a', 'person', 0, 'a'],
  ['solve-4-a', 'person', 0, 'a'],
  ['solve-01-a', 'person', 0, 'a'],
  ['solve-1-control', 'person', 0, 'a'],
  ['judge-1-2', 'brain-analyzator', null, 'system'],
  ['sources-2', 'stalker', 2, 'control'],
  ['profile-2', 'person-fabric', 2, 'control'],
  ['adapt-3', 'person-fabric', 3, 'unknown'],
  ['rubric-extra', 'brain-analyzator', null, 'system'],
  [' solve-1-a', 'person', 0, 'a'],
] as const)(
  'does not invent phase/round for unsupported id %s',
  (logicalStageId, component, person, participant) => {
    const result = classifyExecutionStage({
      logicalStageId,
      component,
      person,
      label: 'Решение · прогон 3',
    });
    expect(result.phase).toBe('unknown');
    expect(result.round).toBeNull();
    expect(result.participant).toBe(participant);
  },
);
it('does not derive identity from labels or conflicting metadata', () => {
  expect(
    classifyExecutionStage({
      logicalStageId: 'legacy',
      component: 'futureComponent',
      person: 8,
      label: 'STALKER Person A прогон 2',
    }),
  ).toEqual({
    participant: 'unknown',
    round: null,
    phase: 'unknown',
    componentLabel: 'futureComponent',
  });
  expect(
    classifyExecutionStage({
      logicalStageId: 'solve-1-a',
      component: 'stalker',
      person: 0,
      label: '...',
    }).phase,
  ).toBe('unknown');
  expect(
    classifyExecutionStage({
      logicalStageId: 'solve-1-a',
      component: 'person',
      person: 1,
      label: '...',
    }).round,
  ).toBeNull();
});
it('uses lifecycle identity when available and the same mapping for legacy StageTrace', () => {
  const lifecycle: StageLifecycle = {
    schemaVersion: 1,
    runId: 'run',
    stageId: 'stage',
    logicalStageId: 'solve-2-neutral',
    component: 'person',
    person: 2,
    label: 'Current',
    status: 'completed',
    startedAt: null,
    finishedAt: null,
    error: null,
    calls: [],
  };
  expect(
    classifyTraceStage({
      id: 'old',
      stage: 'stalker',
      person: 0,
      label: 'Old',
      lifecycle,
    }),
  ).toEqual({
    participant: 'control',
    round: 2,
    phase: 'solution',
    componentLabel: 'Person',
  });
  const legacy = Object.freeze({
    id: 'profile-1',
    stage: 'person-fabric' as const,
    person: 1,
    label: ' Exact\r\nlabel ',
  });
  expect(classifyTraceStage(legacy)).toEqual({
    participant: 'b',
    round: null,
    phase: 'profile',
    componentLabel: 'Person Fabric',
  });
  expect(legacy.label).toBe(' Exact\r\nlabel ');
});
