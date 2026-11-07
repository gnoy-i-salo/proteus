import { describe, expect, it } from 'vitest';
import { decodeModelSettings, encodeModelSettings } from './model-settings-codec';

const components = {
  stalker: 'codex',
  'person-fabric': 'kimi',
  person: 'codex',
  'brain-analyzator': 'kimi',
};
const aliases = {
  stalker: '  Модель 🧠\nwith-dash  ',
  'person-fabric': '',
  person: 'source-url',
  'brain-analyzator': '',
};

describe('model settings boundary', () => {
  it('maps only the owned legacy field and preserves component IDs and exact alias text', () => {
    expect(
      decodeModelSettings(JSON.stringify({ 'global-model': 'codex', components, aliases })),
    ).toEqual({ globalModel: 'codex', components, aliases });
  });

  it.each([
    null,
    [],
    {},
    { 'global-model': 'other', components },
    { 'global-model': 'codex', globalModel: 'kimi', components },
    { 'global-model': 'codex', globalModel: 'codex', components },
    { format: 'proteus-model-settings', version: 99, globalModel: 'codex', components, aliases },
    { 'global-model': 'codex', components: { ...components, 'brain-scoring': 'codex' } },
  ])('rejects invalid or mixed schemas rather than replacing user settings: %j', (value) => {
    expect(() => decodeModelSettings(JSON.stringify(value))).toThrow();
  });

  it('rejects invalid JSON and oversized input before parsing', () => {
    expect(() => decodeModelSettings('{')).toThrow();
    expect(() => decodeModelSettings(' '.repeat(16385))).toThrow();
  });

  it('round trips the new version without rewriting identifier values or text', () => {
    const settings = {
      globalModel: 'codex' as const,
      components: {
        stalker: 'codex' as const,
        'person-fabric': 'kimi' as const,
        person: 'codex' as const,
        'brain-analyzator': 'kimi' as const,
      },
      aliases,
    };
    const encoded = encodeModelSettings(settings);
    expect(JSON.parse(encoded)).toEqual({
      format: 'proteus-model-settings',
      version: 2,
      ...settings,
    });
    expect(decodeModelSettings(encoded)).toEqual(settings);
    expect(encodeModelSettings(decodeModelSettings(encoded))).toBe(encoded);
  });

  it('reads original settings without aliases using the legacy defaults', () => {
    expect(decodeModelSettings(JSON.stringify({ 'global-model': 'kimi', components }))).toEqual({
      globalModel: 'kimi',
      components,
      aliases: { stalker: '', 'person-fabric': '', person: '', 'brain-analyzator': '' },
    });
  });

  it.each([
    { ...aliases, person: null },
    { ...aliases, person: 12 },
    { ...aliases, person: 'я'.repeat(121) },
    { ...aliases, unknown: 'value' },
  ])('rejects corrupt alias records: %j', (invalidAliases) => {
    expect(() =>
      decodeModelSettings(
        JSON.stringify({
          'global-model': 'codex',
          components,
          aliases: invalidAliases,
        }),
      ),
    ).toThrow();
  });

  it('does not mutate a caller-owned input during validation or encoding', () => {
    const settings = Object.freeze({
      globalModel: 'codex' as const,
      components: Object.freeze({
        stalker: 'codex' as const,
        'person-fabric': 'kimi' as const,
        person: 'codex' as const,
        'brain-analyzator': 'kimi' as const,
      }),
      aliases: Object.freeze({ ...aliases }),
    });
    expect(decodeModelSettings(encodeModelSettings(settings))).toEqual(settings);
  });
});
