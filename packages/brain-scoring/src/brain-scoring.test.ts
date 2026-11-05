import { describe, expect, it } from 'vitest';
import { scorePerson } from './index';
describe('BSC from the PROTEUS board', () => {
  it('divides reasoning minutes by estimated BSC; excludes terminal time', () => {
    expect(
      scorePerson({
        'person-id': 'a',
        'reasoning-ms': 120_000,
        'terminal-ms': 600_000,
        'estimated-bsc': 4,
      }),
    ).toEqual({ 'person-id': 'a', 'reasoning-minutes': 2, bsc: 0.5 });
  });
  it('accepts zero reasoning time', () => {
    expect(
      scorePerson({ 'person-id': 'a', 'reasoning-ms': 0, 'terminal-ms': 0, 'estimated-bsc': 2 })
        .bsc,
    ).toBe(0);
  });
  it.each([0, -1, NaN, Infinity])('rejects invalid estimated BSC %s', (value) => {
    expect(() =>
      scorePerson({
        'person-id': 'a',
        'reasoning-ms': 60_000,
        'terminal-ms': 0,
        'estimated-bsc': value,
      }),
    ).toThrow();
  });
  it.each([-1, NaN, Infinity])('rejects invalid duration %s', (value) => {
    expect(() =>
      scorePerson({
        'person-id': 'a',
        'reasoning-ms': value,
        'terminal-ms': 0,
        'estimated-bsc': 1,
      }),
    ).toThrow();
  });
});
