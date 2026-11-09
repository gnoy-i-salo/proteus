import { expect, it } from 'vitest';
import { decodeCorpusEpisodeQuery, decodeCorpusEpisodeResult } from './corpus-inspection-codec';
import fixture from './fixtures/corpus-inspection-wire.json';

it('decodes the pinned inspection scope without adding model or export rights', () => {
  expect(decodeCorpusEpisodeQuery(fixture.query)).toEqual(fixture.query);
});
it('keeps exact source bytes, authorship and relationships in detached inspection results', () => {
  const value = structuredClone(fixture.result);
  const decoded = decodeCorpusEpisodeResult(value);
  expect(decoded).toEqual(value);
  expect(decoded.chunks[0]!.excerpt.text).toBe(
    '  Анна → Борис\r\n«Ёлка» 👩🏽‍🔬\nhttps://example.org/Exact-ID  ',
  );
  value.chunks[0]!.excerpt.text = 'changed';
  expect(decoded.chunks[0]!.excerpt.text).toBe(fixture.result.chunks[0]!.excerpt.text);
});
it.each(['unknown', 'version', 'period', 'generation', 'duration', 'identity'])(
  'rejects invalid inspection queries: %s',
  (variant) => {
    const value = structuredClone(fixture.query);
    if (variant === 'unknown') Object.assign(value, { grantId: 'caller-cannot-grant-access' });
    if (variant === 'version') Object.assign(value, { schemaVersion: 99 });
    if (variant === 'period') Object.assign(value.period, { from: 2, to: 1 });
    if (variant === 'generation') Object.assign(value, { expectedIndexGeneration: null });
    if (variant === 'duration') Object.assign(value, { maxDurationMs: 60001 });
    if (variant === 'identity') Object.assign(value, { personaId: 'foreign\n' });
    expect(() => decodeCorpusEpisodeQuery(value)).toThrow();
  },
);
it.each(['unknown', 'version', 'duplicate', 'bytes', 'empty', 'holdout', 'unicode', 'oversized'])(
  'rejects invalid inspection responses without rewriting originals: %s',
  (variant) => {
    const value = structuredClone(fixture.result);
    if (variant === 'unknown') Object.assign(value, { rawContext: 'private' });
    if (variant === 'version') value.schemaVersion = 99;
    if (variant === 'duplicate') value.chunks.push(value.chunks[0]!);
    if (variant === 'bytes') value.sourceBytes++;
    if (variant === 'empty') value.chunks = [];
    if (variant === 'holdout') value.chunks[0]!.excerpt.document.partition = 'holdout';
    if (variant === 'unicode') value.chunks[0]!.excerpt.text = '\ud800';
    if (variant === 'oversized') value.sourceBytes = 1024 * 1024 + 1;
    const saved = JSON.stringify(value);
    expect(() => decodeCorpusEpisodeResult(value)).toThrow();
    expect(JSON.stringify(value)).toBe(saved);
  },
);
