import { expect, it } from 'vitest';
import { decodeV4Result } from './result-v4';
import type { StoredV4Result } from './result-v4';

import fixture from '../testing/legacy-results/v4-current-with-execution.json';
const cases: readonly [string, (value: StoredV4Result) => void][] = [
  [
    'duplicate participant',
    (v) => Object.assign(v.people[1]!.profile, { id: v.people[0]!.profile.id }),
  ],
  ['base profile identity', (v) => Object.assign(v.people[0]!.baseProfile, { id: 'foreign' })],
  ['summary identity', (v) => Object.assign(v.people[0]!.score, { personId: 'foreign' })],
  [
    'unknown planned participant',
    (v) => Object.assign(v.protocol.participantOrder[0]!, { 0: 'foreign' }),
  ],
  [
    'duplicate planned participant',
    (v) => Object.assign(v.protocol.participantOrder[0]!, { 0: 'b' }),
  ],
  ['duplicate trial', (v) => Object.assign(v.trials[1]!, { id: v.trials[0]!.id })],
  ['wrong trial round', (v) => Object.assign(v.trials[0]!, { round: 2 })],
  ['unknown trial participant', (v) => Object.assign(v.trials[0]!, { personId: 'foreign' })],
  ['trial order', (v) => Object.assign(v.trials, { 0: v.trials[1], 1: v.trials[0] })],
  ['trial score identity', (v) => Object.assign(v.trials[0]!.score!, { personId: 'b' })],
  ['evaluation identity', (v) => Object.assign(v.trials[0]!.evaluations[0]!, { personId: 'b' })],
  [
    'unknown score criterion',
    (v) => Object.assign(v.trials[0]!.score!.criteria[0]!, { criterionId: 'foreign' }),
  ],
  [
    'unknown assessment criterion',
    (v) => Object.assign(v.trials[0]!.evaluations[0]!.criteria[0]!, { criterionId: 'foreign' }),
  ],
  [
    'conflicting trial copy',
    (v) => Object.assign(v.people[0]!.trials[0]!.attempt!, { answer: 'Changed answer' }),
  ],
  ['missing trial copy', (v) => Object.assign(v.people[0]!, { trials: [] })],
  [
    'conflicting selected answer',
    (v) => Object.assign(v.people[0]!.attempt!, { answer: 'Changed answer' }),
  ],
  [
    'duplicate rubric criterion',
    (v) =>
      Object.assign(v.protocol.rubric, {
        criteria: [v.protocol.rubric.criteria[0], v.protocol.rubric.criteria[0]],
      }),
  ],
];
it.each(cases)('rejects %s without repairing the saved file', (_label, change) => {
  const value = decodeV4Result(fixture);
  change(value);
  const before = JSON.stringify(value);
  expect(() => decodeV4Result(value)).toThrow();
  expect(JSON.stringify(value)).toBe(before);
});
