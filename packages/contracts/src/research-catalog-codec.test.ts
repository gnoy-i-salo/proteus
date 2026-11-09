import { expect, it } from 'vitest';
import type { ResearchCatalog, ResearchSessionDescriptor } from './research-memory';
import { decodeResearchCatalog } from './research-catalog-codec';
const session: ResearchSessionDescriptor = {
  schemaVersion: 1,
  id: 'session:1',
  personaId: 'persona:1',
  corpusSnapshotId: 'snapshot:1',
  policyVersion: 'policy:1',
  mode: 'lexical',
  indexGeneration: 2,
  expiresAt: '2099-01-01T00:00:00.000Z',
};
const catalog: ResearchCatalog = {
  schemaVersion: 1,
  snapshotId: session.corpusSnapshotId,
  personaId: session.personaId,
  policyVersion: session.policyVersion,
  indexGeneration: session.indexGeneration,
  eligibleEpisodes: 3,
  samples: [{ episodeId: 'episode:1', sourceKey: 'binding:1', periodKey: 'window30d:-1' }],
};
it('decodes a bounded native catalog without claiming all eligible episodes were returned', () => {
  const result = decodeResearchCatalog(catalog, session, 1);
  expect(result).toEqual(catalog);
  expect(result.samples).not.toBe(catalog.samples);
  expect(
    decodeResearchCatalog({ ...catalog, eligibleEpisodes: 0, samples: [] }, session, 1).samples,
  ).toEqual([]);
  expect(
    decodeResearchCatalog(
      { ...catalog, samples: [{ ...catalog.samples[0], periodKey: 'undated' }] },
      session,
      1,
    ).samples[0]?.periodKey,
  ).toBe('undated');
});
it.each([
  { schemaVersion: 2 },
  { personaId: 'foreign' },
  { snapshotId: 'foreign' },
  { policyVersion: 'foreign' },
  { indexGeneration: 3 },
  { unexpected: 'field' },
  { eligibleEpisodes: 0 },
  { eligibleEpisodes: 100001 },
  { eligibleEpisodes: 1.5 },
  { samples: [catalog.samples[0], catalog.samples[0]] },
  { samples: [] },
  { samples: [{ ...catalog.samples[0], periodKey: '2026-01' }] },
  { samples: [{ ...catalog.samples[0], periodKey: 'window30d:-0' }] },
  { samples: [{ ...catalog.samples[0], sourceKey: '\ud800' }] },
  { samples: [{ ...catalog.samples[0], episodeId: 'a'.repeat(257) }] },
  { samples: [{ ...catalog.samples[0], text: 'must not disclose original text' }] },
])('rejects mismatched, unbounded or malformed metadata %j', (change) => {
  expect(() => decodeResearchCatalog({ ...catalog, ...change }, session, 1)).toThrow();
});
it.each([0, 10001, 0.5, NaN])('rejects invalid requested limit %s', (limit) => {
  expect(() => decodeResearchCatalog(catalog, session, limit)).toThrow();
});
