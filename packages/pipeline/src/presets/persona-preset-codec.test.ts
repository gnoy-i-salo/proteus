import { describe, expect, it } from 'vitest';
import { decodePersonaPreset, encodePersonaPreset } from './persona-preset-codec';

const input = {
  task: '  public-urls и client_id — исходный текст\r\n**Не менять** 😀  ',
  people: [
    {
      label: ' Анна Ёлкина ',
      context: 'Имя: Анна\r\n  «Точный текст»\n@handle — https://example.org/source-id',
      publicUrls: ['https://example.org/Source-ID?client_id=Original-Value#Quote'],
    },
    { label: 'Борис', context: 'public-urls, publicUrls, 🧪', publicUrls: [] },
  ],
} as const;
const legacy = {
  format: 'proteus-personas',
  version: 1,
  task: input.task,
  people: input.people.map((person) => ({
    label: person.label,
    context: person.context,
    'public-urls': person.publicUrls,
  })),
};
const current = { format: 'proteus-personas', version: 2, ...input };

describe('persona preset compatibility boundary', () => {
  it('maps only legacy schema fields to canonical input, preserving exact source text', () => {
    expect(decodePersonaPreset(JSON.stringify(legacy))).toEqual(input);
  });

  it('reads canonical version 2 and writes only that version', () => {
    const data = encodePersonaPreset(input);
    expect(JSON.parse(data)).toEqual(current);
    expect(decodePersonaPreset(data)).toEqual(input);
    expect(decodePersonaPreset(JSON.stringify(current))).toEqual(input);
  });

  it.each(['', ' \t\r\n'])('preserves an optional blank task: %j', (task) => {
    expect(decodePersonaPreset(encodePersonaPreset({ ...input, task }))).toEqual({
      ...input,
      task,
    });
  });

  it.each([
    ['missing version', { ...current, version: undefined }],
    ['unknown version', { ...current, version: 3 }],
    ['wrong format', { ...current, format: 'proteus-template' }],
    ['unknown root field', { ...current, extraField: true }],
    ['imported grants', { ...current, grants: [{ allowModelProcessing: true }] }],
    ['mixed legacy root', { ...legacy, schemaVersion: 2 }],
    ['one person', { ...current, people: [input.people[0]] }],
    ['three people', { ...current, people: [...input.people, input.people[0]] }],
    ['non-string task', { ...current, task: null }],
    ['oversize task', { ...current, task: 'a'.repeat(4001) }],
    ['oversize blank task', { ...current, task: ' '.repeat(4001) }],
    ['null person', { ...current, people: [null, input.people[1]] }],
    ['array person', { ...current, people: [[], input.people[1]] }],
  ])('rejects %s', (_name, data) => {
    expect(() => decodePersonaPreset(JSON.stringify(data))).toThrow();
  });

  it.each([
    [
      'legacy and canonical aliases',
      { ...input.people[0], 'public-urls': input.people[0].publicUrls },
    ],
    ['conflicting aliases', { ...input.people[0], 'public-urls': ['https://example.org/other'] }],
    ['unknown person field', { ...input.people[0], authorId: 'someone' }],
    ['per-person grant', { ...input.people[0], grants: [] }],
    ['missing URLs', { label: 'Анна', context: 'контекст' }],
    ['null URLs', { ...input.people[0], publicUrls: null }],
    ['non-string URL', { ...input.people[0], publicUrls: [false] }],
    ['private URL', { ...input.people[0], publicUrls: ['https://127.0.0.1/private'] }],
    ['file URL', { ...input.people[0], publicUrls: ['file:///etc/passwd'] }],
    [
      'too many URLs',
      { ...input.people[0], publicUrls: Array.from({ length: 6 }, () => 'https://example.org') },
    ],
    ['blank name', { ...input.people[0], label: ' \t' }],
    ['oversize name', { ...input.people[0], label: 'a'.repeat(81) }],
    ['oversize context', { ...input.people[0], context: 'a'.repeat(4001) }],
    ['no usable context', { ...input.people[0], context: ' \t', publicUrls: [] }],
  ])('rejects %s in canonical input', (_name, person) => {
    expect(() =>
      decodePersonaPreset(JSON.stringify({ ...current, people: [person, input.people[1]] })),
    ).toThrow();
  });

  it('rejects canonical fields masquerading as version 1, including equal aliases', () => {
    expect(() => decodePersonaPreset(JSON.stringify({ ...current, version: 1 }))).toThrow();
    expect(() =>
      decodePersonaPreset(
        JSON.stringify({
          ...legacy,
          people: [
            { ...legacy.people[0], publicUrls: input.people[0].publicUrls },
            legacy.people[1],
          ],
        }),
      ),
    ).toThrow();
  });

  it.each(['not JSON', 'null', '[]', '{}', ' '.repeat(131073)])(
    'rejects malformed/oversize input',
    (data) => {
      expect(() => decodePersonaPreset(data)).toThrow();
    },
  );

  it('measures the input budget in UTF-8 bytes rather than character count', () => {
    const data = JSON.stringify({ ...legacy, padding: 'я'.repeat(66000) });
    expect(data.length).toBeLessThan(131072);
    expect(() => decodePersonaPreset(data)).toThrow(/128/);
  });

  it('rejects unknown writer fields instead of exporting grants or credentials', () => {
    const withGrants = { ...input, grants: [] };
    expect(() => encodePersonaPreset(withGrants)).toThrow();
  });

  it('validates the writer instead of serializing an invalid persona', () => {
    expect(() => encodePersonaPreset({ ...input, task: 'x'.repeat(4001) })).toThrow();
  });
});
