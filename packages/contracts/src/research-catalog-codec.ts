import type { ResearchCatalog, ResearchSessionDescriptor } from './research-memory';
import { fields, integer } from './corpus-wire-validation';
import { memoryId } from './memory-wire-validation';
export function decodeResearchCatalog(
  value: unknown,
  session: ResearchSessionDescriptor,
  limit: number,
): ResearchCatalog {
  const input = fields(value, [
    'schemaVersion',
    'snapshotId',
    'indexGeneration',
    'personaId',
    'policyVersion',
    'eligibleEpisodes',
    'samples',
  ]);
  if (
    !Number.isSafeInteger(limit) ||
    limit < 1 ||
    limit > 10000 ||
    input.schemaVersion !== 1 ||
    input.snapshotId !== session.corpusSnapshotId ||
    input.indexGeneration !== session.indexGeneration ||
    input.personaId !== session.personaId ||
    input.policyVersion !== session.policyVersion
  )
    throw new Error('Каталог не соответствует исследовательской сессии.');
  const eligibleEpisodes = integer(input.eligibleEpisodes, 0);
  if (
    eligibleEpisodes > 100000 ||
    !Array.isArray(input.samples) ||
    input.samples.length !== Math.min(limit, eligibleEpisodes)
  )
    throw new Error('Неверный размер каталога исследования.');
  const seen = new Set<string>();
  const samples = input.samples.map((value: unknown) => {
    const sample = fields(value, ['episodeId', 'sourceKey', 'periodKey']);
    const episodeId = memoryId(sample.episodeId);
    const sourceKey = memoryId(sample.sourceKey);
    const periodKey = memoryId(sample.periodKey, 64);
    if (
      seen.has(episodeId) ||
      (periodKey !== 'undated' && !/^window30d:(?:0|-?[1-9]\d{0,6})$/u.test(periodKey))
    )
      throw new Error('Неверный эпизод каталога исследования.');
    seen.add(episodeId);
    return { episodeId, sourceKey, periodKey };
  });
  return {
    schemaVersion: 1,
    snapshotId: session.corpusSnapshotId,
    indexGeneration: session.indexGeneration,
    personaId: session.personaId,
    policyVersion: session.policyVersion,
    eligibleEpisodes,
    samples,
  };
}
