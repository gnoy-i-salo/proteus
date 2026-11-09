import { describe, expect, it } from 'vitest';
import { decodePersonaManifest, decodeSkillDefinition } from './memory-profile-codec';
import { frozenManifest } from './testing/manifest-fixtures';

describe('persisted manifest and skill readers', () => {
  it('preserves native V1 identity, exact skill words and nullable model settings', () => {
    const { manifest, skills } = frozenManifest();
    expect(decodePersonaManifest(manifest)).toEqual(manifest);
    expect(decodeSkillDefinition(skills[0])).toEqual(skills[0]);
    const legacy = {
      ...manifest,
      createdAt: '2026-11-08T20:00:00.123456789+03:00',
      modelSettings: manifest.modelSettings.map((entry) => ({ ...entry, requestedModel: null })),
    };
    expect(decodePersonaManifest(legacy)).toEqual(legacy);
  });
  it('requires every persisted field and rejects aliases or unexpected fields', () => {
    for (const [value, decode] of [
      [frozenManifest().manifest, decodePersonaManifest],
      [frozenManifest().skills[0]!, decodeSkillDefinition],
    ] as const) {
      for (const key of Object.keys(value)) {
        const input: Record<string, unknown> = { ...value };
        delete input[key];
        expect(() => decode(input)).toThrow();
      }
      for (const patch of [
        { schemaVersion: 2 },
        { unknown: true },
        Object.fromEntries([['source-spans', []]]),
      ])
        expect(() => decode({ ...value, ...patch })).toThrow();
    }
  });
  it.each([
    { id: '' },
    { trigger: ' ' },
    { trigger: 'я'.repeat(2049) },
    { trigger: '\ud800' },
    { inputs: [' '] },
    { inputs: Array(17).fill('input') },
    { steps: [] },
    { steps: ['я'.repeat(1025)] },
    { outcomeCheck: '' },
    { support: 'unknown' },
    { limitations: [] },
    { limitations: Array(17).fill('x') },
    { memoryItemIds: [] },
    { memoryItemIds: ['a', 'a'] },
    { memoryItemIds: Array.from({ length: 17 }, (_, i) => `id:${i}`) },
    { sourceSpans: [] },
  ])('rejects malformed or over-budget skills: %j', (patch) => {
    const input = { ...frozenManifest().skills[0], ...patch };
    const before = structuredClone(input);
    expect(() => decodeSkillDefinition(input)).toThrow();
    expect(input).toEqual(before);
  });
  it('accepts observed skills structurally without claiming native source proof', () => {
    const input = { ...frozenManifest().skills[0]!, support: 'observed', limitations: [] };
    expect(decodeSkillDefinition(input)).toEqual(input);
    const span = input.sourceSpans[0]!;
    for (const spans of [
      [span, span],
      [{ ...span, end: 0 }],
      [{ ...span, documentId: 'x\n' }],
      [{ ...span, revisionId: 'я'.repeat(129) }],
    ])
      expect(() => decodeSkillDefinition({ ...input, sourceSpans: spans })).toThrow();
  });
  it.each([
    { id: 'x\n' },
    { personaId: '' },
    { profileVersion: '' },
    { corpusSnapshotId: '' },
    { memorySnapshotId: '' },
    { skillIds: ['a', 'a'] },
    { skillIds: Array.from({ length: 65 }, (_, i) => `id:${i}`) },
    { exemplarIds: Array.from({ length: 101 }, (_, i) => `id:${i}`) },
    { mode: 'hybrid' },
    { splitVersion: '' },
    { preparationVersion: '' },
    { retrievalVersion: '' },
    { promptVersion: '' },
    { createdAt: '2026-02-30T00:00:00Z' },
    { modelSettings: [] },
    { sourceRefs: [{ documentId: 'd' }] },
  ])('rejects malformed frozen manifests: %j', (patch) => {
    expect(() => decodePersonaManifest({ ...frozenManifest().manifest, ...patch })).toThrow();
  });
  it('rejects repeated sources, duplicate or unknown components, malformed model settings and budgets', () => {
    const { manifest } = frozenManifest();
    const model = manifest.modelSettings[0]!;
    expect(() =>
      decodePersonaManifest({
        ...manifest,
        sourceRefs: [...manifest.sourceRefs, ...manifest.sourceRefs],
      }),
    ).toThrow();
    expect(() =>
      decodePersonaManifest({ ...manifest, modelSettings: Array(4).fill(model) }),
    ).toThrow();
    for (const patch of [
      { component: 'judge' },
      { provider: 'cloud' },
      { requestedModel: '' },
      { requestedModel: 'я'.repeat(61) },
      { requestedEffort: 'max' },
      { unknown: true },
    ])
      expect(() =>
        decodePersonaManifest({
          ...manifest,
          modelSettings: [{ ...model, ...patch }, ...manifest.modelSettings.slice(1)],
        }),
      ).toThrow();
    for (const field of ['recallBudget', 'modelCallBudget'] as const)
      expect(() =>
        decodePersonaManifest({ ...manifest, [field]: { ...manifest[field], maxCalls: 0 } }),
      ).toThrow();
  });
  it('uses native UTF8 template limits for fingerprints without changing legacy receipt readers', () => {
    const fingerprint = {
      modelId: 'm',
      modelRevision: 'r',
      modelSha256: 'a'.repeat(64),
      tokenizerSha256: 'b'.repeat(64),
      dimensions: 384,
      pooling: 'mean',
      normalization: 'l2',
      queryTemplate: 'q'.repeat(4096),
      passageTemplate: 'я'.repeat(2048),
      chunkingVersion: 'chunk:1',
    };
    const value = { ...frozenManifest().manifest, embeddingFingerprint: fingerprint };
    expect(decodePersonaManifest(value)).toEqual(value);
    for (const patch of [
      { dimensions: 4097 },
      { modelId: 'x\n' },
      { modelSha256: 'A'.repeat(64) },
      { queryTemplate: '' },
      { passageTemplate: 'я'.repeat(2049) },
    ])
      expect(() =>
        decodePersonaManifest({ ...value, embeddingFingerprint: { ...fingerprint, ...patch } }),
      ).toThrow();
  });
});
