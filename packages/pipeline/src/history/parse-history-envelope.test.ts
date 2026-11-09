import { describe, expect, it } from 'vitest';
import v1 from '../testing/legacy-results/v1-unversioned-result.json';
import v2 from '../testing/legacy-results/v2-kebab-result.json';
import v3 from '../testing/legacy-results/v3-kebab-export.json';
import v4 from '../testing/legacy-results/v4-camel-no-execution.json';
import current from '../testing/legacy-results/v4-current-with-execution.json';
import { parseHistoryEnvelope } from './parse-history-envelope';

function changed(source: object, key: string, value: unknown): string {
  const result: Record<string, unknown> = { ...source };
  result[key] = value;
  return JSON.stringify(result);
}

describe('saved history envelope routing', () => {
  it.each([
    [1, v1],
    [2, v2],
    [3, v3],
    [4, v4],
    [4, current],
  ] as const)('routes format %s without changing historical data', (formatVersion, fixture) => {
    const original = JSON.stringify(fixture);
    const decoded = parseHistoryEnvelope(` \r\n${original}\t `);
    expect(decoded).toEqual({ formatVersion, value: fixture });
    expect(JSON.stringify(fixture)).toBe(original);
    expect(decoded.value).not.toBe(fixture);
  });

  it('preserves exact source strings and opaque map keys', () => {
    const fixture = structuredClone(v1);
    fixture.input.task = ' \ufeffАвтор e\u0301 😀\r\n\0  ';
    Object.defineProperty(fixture.people[0]!.artifacts, '__proto__', {
      value: 'opaque data',
      enumerable: true,
    });
    const decoded = parseHistoryEnvelope(JSON.stringify(fixture));
    expect(decoded.value).toEqual(fixture);
    expect(Object.getPrototypeOf(decoded.value)).toBe(Object.prototype);
    expect({}).not.toHaveProperty('polluted');
  });

  it.each([
    'null',
    '[]',
    '"result"',
    '{}',
    '{broken',
    JSON.stringify({ ...v1, schemaVersion: 1 }),
    changed(v1, 'schema-version', 1),
    JSON.stringify({ ...v1, finishedAt: v1['finished-at'] }),
    JSON.stringify({ ...v2, schemaVersion: 2 }),
    JSON.stringify({ ...v3, schemaVersion: 3 }),
    changed(v4, 'schema-version', 4),
    JSON.stringify({ ...v4, schemaVersion: 99 }),
    JSON.stringify({ ...v4, schemaVersion: '4' }),
    JSON.stringify({ ...v4, scoringVersion: 99 }),
    changed(v3, 'scoring-version', 99),
  ])('rejects ambiguous, unknown or corrupt envelopes', (text) => {
    expect(() => parseHistoryEnvelope(text)).toThrow();
  });

  it('rejects a BOM outside JSON and invalid literal UTF-8 before decoding', () => {
    expect(() => parseHistoryEnvelope(`\ufeff${JSON.stringify(v1)}`)).toThrow();
    expect(() => parseHistoryEnvelope(JSON.stringify(v1).replace('Design', '\ud800'))).toThrow();
  });

  it('accepts exactly 8 MiB and rejects one extra UTF-8 byte', () => {
    const original = JSON.stringify(v1);
    const size = new TextEncoder().encode(original).length;
    const exact = original + ' '.repeat(8 * 1024 * 1024 - size);
    expect(parseHistoryEnvelope(exact).value).toEqual(v1);
    expect(() => parseHistoryEnvelope(`${exact} `)).toThrow(/8 МиБ/u);
    expect(() => parseHistoryEnvelope(`${exact}я`)).toThrow(/8 МиБ/u);
  });

  it('does not mistake envelope routing for validation of a complete result', () => {
    const malformedBody = { ...v4, people: null };
    expect(parseHistoryEnvelope(JSON.stringify(malformedBody))).toEqual({
      formatVersion: 4,
      value: malformedBody,
    });
  });
});
