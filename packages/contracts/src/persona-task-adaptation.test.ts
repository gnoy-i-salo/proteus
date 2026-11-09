import { expect, it } from 'vitest';
import { decodePersonaTaskAdaptation } from './persona-task-adaptation';
const scope = {
  manifestId: 'manifest:1',
  task: '\uFEFFНапиши Борису é 👩🏽‍🔬\r\n',
  profile: {
    sourceStatus: 'evidence-informed' as const,
    rules: [
      {
        id: 'r1',
        instruction: 'base',
        evidenceIds: ['m1'],
        status: 'supported' as const,
        limitations: ['limited context'],
      },
      {
        id: 'r2',
        instruction: 'tentative',
        evidenceIds: ['m2'],
        status: 'hypothesis' as const,
        limitations: ['uncertain'],
      },
    ],
  },
};
const adaptation = {
  schemaVersion: 1,
  manifestId: scope.manifestId,
  task: scope.task,
  strategies: [
    {
      id: 's1',
      instruction: '  Сначала уточни\r\n',
      evidenceIds: ['m1'],
      status: 'hypothesis',
      limitations: ['limited context'],
    },
  ],
};
it('keeps exact task and strategy text in a detached per-task value', () => {
  const input = structuredClone(adaptation);
  const decoded = decodePersonaTaskAdaptation(input, scope);
  expect(decoded).toEqual(adaptation);
  input.strategies[0]!.instruction = 'changed';
  expect(decoded.strategies[0]!.instruction).toBe(adaptation.strategies[0]!.instruction);
});
it('permits general tentative strategies for a neutral participant', () => {
  const input = {
    ...adaptation,
    strategies: [{ ...adaptation.strategies[0], evidenceIds: [], limitations: [] }],
  };
  expect(
    decodePersonaTaskAdaptation(input, {
      ...scope,
      profile: { sourceStatus: 'neutral', rules: [] },
    }),
  ).toEqual(input);
});
it.each([
  { ...adaptation, schemaVersion: 2 },
  { ...adaptation, manifestId: 'other' },
  { ...adaptation, task: scope.task.trim() },
  { ...adaptation, futureTask: 'other' },
  { ...adaptation, strategies: [] },
  {
    ...adaptation,
    strategies: Array.from({ length: 6 }, (_, i) => ({ ...adaptation.strategies[0], id: `s${i}` })),
  },
  { ...adaptation, strategies: [adaptation.strategies[0], adaptation.strategies[0]] },
  ...[
    { evidenceIds: ['foreign'] },
    { evidenceIds: ['m1', 'm1'] },
    { limitations: [] },
    { evidenceIds: ['m2'], limitations: ['uncertain'], status: 'supported' },
    { evidenceIds: [], status: 'supported' },
    { status: 'unknown' },
    { id: 'r1' },
    { instruction: '' },
    { instruction: '\ud800' },
    { extra: true },
  ].map((change) => ({ ...adaptation, strategies: [{ ...adaptation.strategies[0], ...change }] })),
])('rejects mismatched, malformed or overstated task guidance (%#)', (input) => {
  expect(() => decodePersonaTaskAdaptation(input, scope)).toThrow();
});
it('does not turn a supported source observation into a proved task strategy', () => {
  expect(() =>
    decodePersonaTaskAdaptation(
      { ...adaptation, strategies: [{ ...adaptation.strategies[0], status: 'supported' }] },
      scope,
    ),
  ).toThrow();
});
