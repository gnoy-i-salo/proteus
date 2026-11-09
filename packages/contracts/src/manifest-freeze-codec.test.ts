import { describe, expect, it } from 'vitest';
import { decodeManifestFreeze, decodePersonaManifestDraft } from './manifest-freeze-codec';
import { frozenManifest, manifestDraft } from './testing/manifest-fixtures';
import { fabricSession } from './testing/fabric-fixtures';

function decode(value: unknown) {
  return decodeManifestFreeze(
    value,
    fabricSession,
    manifestDraft(),
    frozenManifest().manifest.sourceRefs,
  );
}
describe('native manifest freeze request and receipt', () => {
  it('preserves exact requested skill text and returned native identities', () => {
    const draft = manifestDraft();
    const receipt = frozenManifest();
    const before = structuredClone({ draft, receipt });
    expect(decodePersonaManifestDraft(draft)).toEqual(draft);
    expect(decode(receipt)).toEqual(receipt);
    expect({ draft, receipt }).toEqual(before);
  });
  it.each([
    { schemaVersion: 2 },
    { contextLeaseIds: [] },
    { contextLeaseIds: ['x', 'x'] },
    { memorySnapshotId: '' },
    { exemplarIds: ['x', 'x'] },
    { mode: 'hybrid' },
    { id: 'forged' },
    { personaId: 'foreign' },
    { corpusSnapshotId: 'foreign' },
    { sourceRefs: [] },
    { preparationVersion: 'forged' },
    { splitVersion: 'forged' },
    { embeddingFingerprint: null },
    { createdAt: '2026-11-09T00:00:00.000Z' },
  ])('rejects malformed or authority-bearing drafts: %j', (patch) => {
    expect(() => decodePersonaManifestDraft({ ...manifestDraft(), ...patch })).toThrow();
  });
  it('rejects missing request fields and duplicate skill drafts', () => {
    const draft = manifestDraft();
    for (const key of Object.keys(draft)) {
      const input: Record<string, unknown> = { ...draft };
      delete input[key];
      expect(() => decodePersonaManifestDraft(input)).toThrow();
    }
    for (const skills of [
      [draft.skills[0], draft.skills[0]],
      Array(65).fill(draft.skills[0]),
      [{ ...draft.skills[0], id: 'forged' }],
    ])
      expect(() => decodePersonaManifestDraft({ ...draft, skills })).toThrow();
  });
  it.each([
    { personaId: 'foreign' },
    { corpusSnapshotId: 'foreign' },
    { memorySnapshotId: 'foreign' },
    { profileVersion: 'other' },
    { promptVersion: 'other' },
    { mode: 'fixedContext' },
    { exemplarIds: [] },
    { sourceRefs: [] },
    { skillIds: [] },
    { retrievalVersion: 'other' },
    { preparationVersion: 'other' },
    { createdAt: fabricSession.expiresAt },
    { createdAt: '2026-11-09T03:00:00Z' },
  ])('rejects a mismatched native manifest: %j', (patch) => {
    const input = frozenManifest();
    expect(() => decode({ ...input, manifest: { ...input.manifest, ...patch } })).toThrow();
  });
  it('binds all future budgets and model settings exactly to the host draft', () => {
    const input = frozenManifest();
    for (const field of ['recallBudget', 'modelCallBudget'] as const)
      expect(() =>
        decode({
          ...input,
          manifest: { ...input.manifest, [field]: { ...input.manifest[field], maxCalls: 1 } },
        }),
      ).toThrow();
    expect(() =>
      decode({
        ...input,
        manifest: {
          ...input.manifest,
          modelSettings: input.manifest.modelSettings.map((entry) => ({
            ...entry,
            requestedModel: 'other',
          })),
        },
      }),
    ).toThrow();
  });
  it('rejects changed skill content, extra skills, reordered identity and outside source references', () => {
    const input = frozenManifest();
    const skill = input.skills[0]!;
    for (const changed of [
      { ...skill, steps: ['changed'] },
      { ...skill, support: 'observed' },
      { ...skill, memoryItemIds: ['foreign'] },
      { ...skill, id: 'other' },
    ])
      expect(() => decode({ ...input, skills: [changed] })).toThrow();
    expect(() => decode({ ...input, skills: [] })).toThrow();
    expect(() => decode({ ...input, skills: [skill, skill] })).toThrow();
    expect(() =>
      decode({
        ...input,
        manifest: {
          ...input.manifest,
          sourceRefs: [{ ...input.manifest.sourceRefs[0]!, revisionId: 'foreign' }],
        },
      }),
    ).toThrow();
    expect(() => decode({ ...input, unknown: true })).toThrow();
    expect(() => decode({ ...input, schemaVersion: 2 })).toThrow();
  });
  it('accepts an empty preparation without inventing skills or cited sources', () => {
    const draft = { ...manifestDraft(), skills: [], exemplarIds: [] };
    const input = frozenManifest();
    const empty = {
      ...input,
      skills: [],
      manifest: { ...input.manifest, skillIds: [], exemplarIds: [], sourceRefs: [] },
    };
    expect(decodeManifestFreeze(empty, fabricSession, draft, [])).toEqual(empty);
  });
});
