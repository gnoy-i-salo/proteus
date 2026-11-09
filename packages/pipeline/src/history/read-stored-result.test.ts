import { expect, it } from 'vitest';
import v1 from '../testing/legacy-results/v1-unversioned-result.json';
import v4 from '../testing/legacy-results/v4-current-with-execution.json';
import { readStoredResult } from './read-stored-result';

it('routes the original score version without inventing a modern protocol', () => {
  const value = readStoredResult(JSON.stringify(v1));
  expect(value.sourceVersion).toBe(1);
  expect(value.result).not.toHaveProperty('protocol');
  expect(value.result.people[0]?.score).toMatchObject({ bsc: v1.people[0]!.score.bsc });
});
it('validates the full modern body, preserving its source version and observations', () => {
  expect(readStoredResult(JSON.stringify(v4))).toEqual({ sourceVersion: 4, result: v4 });
  expect(() => readStoredResult(JSON.stringify({ ...v4, trials: [] }))).toThrow();
});
it('rejects unsupported or mixed envelopes without partial output', () => {
  for (const value of [
    {},
    { ...v4, schemaVersion: 99 },
    Object.fromEntries([...Object.entries(v4), ['schema-version', 3]]),
  ])
    expect(() => readStoredResult(JSON.stringify(value))).toThrow();
});
