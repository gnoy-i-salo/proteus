import type { CorpusEpisodeQuery, CorpusEpisodeResult } from './corpus-inspection';
import { fields, flag, id, integer, list } from './corpus-wire-validation';
import { decodeSourceExcerpt } from './source-excerpt-codec';
const encoder = new TextEncoder();
function scopeId(value: unknown, maxBytes = 256): string {
  const selected = id(value);
  const bytes = encoder.encode(selected);
  if (
    bytes.length > maxBytes ||
    /\p{Cc}/u.test(selected) ||
    new TextDecoder('utf-8', { fatal: true, ignoreBOM: true }).decode(bytes) !== selected
  )
    throw new Error('Неверный идентификатор просмотра источника.');
  return selected;
}
function bounded(value: unknown, min: number, max: number): number {
  const selected = integer(value, min);
  if (selected > max) throw new Error('Превышен предел просмотра источника.');
  return selected;
}
export function decodeCorpusEpisodeQuery(value: unknown): CorpusEpisodeQuery {
  const input = fields(value, [
    'jobId',
    'snapshotId',
    'personaId',
    'episodeId',
    'period',
    'expectedIndexGeneration',
    'maxDurationMs',
  ]);
  const period = fields(input.period, ['from', 'to', 'includeUndated']);
  const from = period.from === null ? null : integer(period.from, -Number.MAX_SAFE_INTEGER);
  const to = period.to === null ? null : integer(period.to, -Number.MAX_SAFE_INTEGER);
  if (from !== null && to !== null && from > to) throw new Error('Неверный период просмотра.');
  return {
    jobId: scopeId(input.jobId, 128),
    snapshotId: scopeId(input.snapshotId),
    personaId: scopeId(input.personaId),
    episodeId: scopeId(input.episodeId),
    period: { from, to, includeUndated: flag(period.includeUndated) },
    expectedIndexGeneration: integer(input.expectedIndexGeneration, 1),
    maxDurationMs: bounded(input.maxDurationMs, 1, 60000),
  };
}
export function decodeCorpusEpisodeResult(value: unknown): CorpusEpisodeResult {
  const input = fields(value, [
    'schemaVersion',
    'snapshotId',
    'indexGeneration',
    'personaId',
    'episodeId',
    'sourceBytes',
    'contextTokens',
    'chunks',
  ]);
  if (input.schemaVersion !== 1) throw new Error('Неизвестная версия просмотра источника.');
  const values = list(input.chunks);
  if (!values.length || values.length > 100)
    throw new Error('Неверное число фрагментов источника.');
  const sourceBytes = bounded(input.sourceBytes, 1, 1024 * 1024);
  let actualBytes = 0;
  const ids = new Set<string>();
  const chunks = values.map((value) => {
    const chunk = fields(value, ['chunkId', 'excerpt']);
    const chunkId = scopeId(chunk.chunkId);
    if (ids.has(chunkId)) throw new Error('Повторный фрагмент источника.');
    ids.add(chunkId);
    const excerpt = decodeSourceExcerpt(chunk.excerpt);
    actualBytes += encoder.encode(excerpt.text).length;
    if (actualBytes > sourceBytes || !excerpt.sourceMetadata)
      throw new Error('Неверный размер или метаданные выдержки.');
    return { chunkId, excerpt };
  });
  if (actualBytes !== sourceBytes) throw new Error('Размер исходного текста не совпадает.');
  return {
    schemaVersion: 1,
    snapshotId: scopeId(input.snapshotId),
    indexGeneration: integer(input.indexGeneration, 1),
    personaId: scopeId(input.personaId),
    episodeId: scopeId(input.episodeId),
    sourceBytes,
    contextTokens: bounded(input.contextTokens, 0, 1000000),
    chunks,
  };
}
