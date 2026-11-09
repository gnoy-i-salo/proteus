import { describe, expect, it } from 'vitest';
import { decodeMemoryItem } from './memory-item-codec';

const item = () => ({
  schemaVersion: 1,
  id: 'memory:1',
  snapshotId: 'corpus:1',
  kind: 'styleExample',
  text: '\uFEFF  Анна → Борис\r\nЁлка 👩🏽‍🔬  ',
  sourceSpans: [{ documentId: 'doc:1', revisionId: 'rev:1', unit: 'utf8Bytes', start: 0, end: 12 }],
  episodeIds: ['episode:1'],
  eventAt: null,
  observedAt: '2026-11-08T20:00:00Z',
  support: 'reported',
  contradictionIds: [],
  limitations: ['  Только слова автора\r\n'],
  extractionVersion: 'extract:1',
  policyVersion: 'policy:1',
});

describe('persisted memory item IPC reader', () => {
  it('preserves exact source words, nullable dates, order and independent output', () => {
    const input = item();
    const before = structuredClone(input);
    const result = decodeMemoryItem(input);
    expect(result).toEqual(input);
    Reflect.set(result.episodeIds, 1, 'other');
    Reflect.set(result.sourceSpans[0]!, 'start', 1);
    expect(input).toEqual(before);
  });

  it.each([
    '2026-11-08T20:00:00Z',
    '2026-11-08T20:00:00.004Z',
    '2026-11-08T20:00:00.123456789+03:00',
    '2024-02-29T23:59:60Z',
    '0000-02-29T00:00:00-00:00',
    '2026-11-08t20:00:00z',
  ])('retains valid native RFC3339 spelling: %s', (timestamp) => {
    const input = { ...item(), eventAt: timestamp, observedAt: timestamp };
    expect(decodeMemoryItem(input)).toEqual(input);
  });

  it.each([
    '2026-02-29T00:00:00Z',
    '2026-04-31T00:00:00Z',
    '2026-00-01T00:00:00Z',
    '2026-11-08',
    '2026-11-08T24:00:00Z',
    '2026-11-08T20:60:00Z',
    '2026-11-08T20:00:61Z',
    '2026-11-08T20:00:00',
    '2026-11-08T20:00:00+24:00',
    '2026-11-08T20:00:00+00:60',
    '2026-11-08T20:00:00.Z',
    ' 2026-11-08T20:00:00Z',
  ])('rejects invalid dates without changing originals: %s', (timestamp) => {
    for (const field of ['eventAt', 'observedAt']) {
      const input = { ...item(), [field]: timestamp };
      const before = structuredClone(input);
      expect(() => decodeMemoryItem(input)).toThrow();
      expect(input).toEqual(before);
    }
  });

  it('requires every versioned field and rejects unknown or legacy aliases', () => {
    for (const key of Object.keys(item())) {
      const input: Record<string, unknown> = item();
      delete input[key];
      expect(() => decodeMemoryItem(input)).toThrow();
    }
    for (const patch of [
      { schemaVersion: 2 },
      Object.fromEntries([['snapshot-id', 'corpus:1']]),
      { own: true },
    ])
      expect(() => decodeMemoryItem({ ...item(), ...patch })).toThrow();
  });

  it.each([
    { id: '' },
    { id: 'bad\n' },
    { id: 'я'.repeat(129) },
    { snapshotId: null },
    { kind: 'skill' },
    { support: 'supported' },
    { text: '' },
    { text: '\ud800' },
    { text: 'я'.repeat(32769) },
    { observedAt: null },
    { extractionVersion: '' },
    { policyVersion: '' },
    { episodeIds: [] },
    { episodeIds: ['a', 'a'] },
    { episodeIds: Array.from({ length: 65 }, (_, i) => `episode:${i}`) },
    { contradictionIds: ['a', 'a'] },
    { contradictionIds: Array.from({ length: 65 }, (_, i) => `item:${i}`) },
    { limitations: ['   '] },
    { limitations: ['я'.repeat(1025)] },
    { limitations: Array(17).fill('limitation') },
    { sourceSpans: [] },
  ])('rejects malformed or over-budget items: %j', (patch) => {
    expect(() => decodeMemoryItem({ ...item(), ...patch })).toThrow();
  });

  it('bounds and validates unique spans using native identifier and integer limits', () => {
    const source = item().sourceSpans[0]!;
    for (const patch of [
      { documentId: 'bad\0' },
      { revisionId: 'я'.repeat(129) },
      { unit: 'characters' },
      { start: -1 },
      { end: 0 },
      { end: 0.5 },
      { end: Number.MAX_SAFE_INTEGER + 1 },
    ])
      expect(() =>
        decodeMemoryItem({ ...item(), sourceSpans: [{ ...source, ...patch }] }),
      ).toThrow();
    expect(() => decodeMemoryItem({ ...item(), sourceSpans: [source, { ...source }] })).toThrow();
    const spans = Array.from({ length: 64 }, (_, i) => ({ ...source, start: i, end: i + 1 }));
    expect(decodeMemoryItem({ ...item(), sourceSpans: spans }).sourceSpans).toEqual(spans);
    expect(() =>
      decodeMemoryItem({ ...item(), sourceSpans: [...spans, { ...source, end: 100 }] }),
    ).toThrow();
    expect(decodeMemoryItem({ ...item(), text: 'я'.repeat(32768) }).text.length).toBe(32768);
  });
});
