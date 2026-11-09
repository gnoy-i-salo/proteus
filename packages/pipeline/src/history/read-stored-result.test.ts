import { expect, it } from 'vitest';
import v1 from '../testing/legacy-results/v1-unversioned-result.json';
import v2 from '../testing/legacy-results/v2-kebab-result.json';
import v3 from '../testing/legacy-results/v3-kebab-export.json';
import v4 from '../testing/legacy-results/v4-current-with-execution.json';
import { readStoredResult } from './read-stored-result';

for (const fixture of [v2, v3])
  it(`opens schema ${fixture['schema-version']} without changing the historical protocol or scores`, () => {
    const before = JSON.stringify(fixture);
    const stored = readStoredResult(before);
    expect(stored.sourceVersion).toBe(fixture['schema-version']);
    expect(stored.result).toMatchObject({
      schemaVersion: fixture['schema-version'],
      protocol: { version: 2, promptVersion: fixture.protocol['prompt-version'] },
      comparison: { summary: fixture.comparison.summary },
      people: fixture.people.map((person) => ({ score: { point: person.score.point } })),
    });
    expect(JSON.stringify(fixture)).toBe(before);
  });

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
