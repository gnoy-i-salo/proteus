import { describe, expect, it } from 'vitest';
import { scorePerson } from './index';
describe('BSC from the PROTEUS board', () => {
  it('divides reasoning minutes by estimated BSC; excludes terminal time', () => {
    expect(
      scorePerson({
        personId: 'a',
        reasoningMs: 120_000,
        terminalMs: 600_000,
        estimatedBsc: 4,
      }),
    ).toEqual({ personId: 'a', reasoningMinutes: 2, bsc: 0.5 });
  });
  it('accepts zero reasoning time', () => {
    expect(scorePerson({ personId: 'a', reasoningMs: 0, terminalMs: 0, estimatedBsc: 2 }).bsc).toBe(
      0,
    );
  });
  it.each([0, -1, NaN, Infinity])('rejects invalid estimated BSC %s', (value) => {
    expect(() =>
      scorePerson({
        personId: 'a',
        reasoningMs: 60_000,
        terminalMs: 0,
        estimatedBsc: value,
      }),
    ).toThrow();
  });
  it.each([-1, NaN, Infinity])('rejects invalid duration %s', (value) => {
    expect(() =>
      scorePerson({
        personId: 'a',
        reasoningMs: value,
        terminalMs: 0,
        estimatedBsc: 1,
      }),
    ).toThrow();
  });
});
