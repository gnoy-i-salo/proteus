import { expect, it } from 'vitest';
import { parsePersonaPreset, serializePersonaPreset } from './index';
const input = {
  task: '',
  people: [
    {
      label: 'Анна',
      context: 'Точный контекст\nс переносом ✨',
      publicUrls: ['https://example.org/a'],
    },
    { label: 'Борис', context: 'Другой подход', publicUrls: [] },
  ],
} as const;
it('round trips two persona inputs, Unicode and optional task without settings or results', () => {
  const data = serializePersonaPreset(input);
  expect(JSON.parse(data)).toEqual({
    format: 'proteus-personas',
    version: 1,
    task: input.task,
    people: input.people.map((person) => ({
      label: person.label,
      context: person.context,
      'public-urls': person.publicUrls,
    })),
  });
  expect(parsePersonaPreset(data)).toEqual(input);
});
it.each([
  'not JSON',
  '{}',
  JSON.stringify({ format: 'proteus-personas', version: 3, ...input }),
  JSON.stringify({ format: 'proteus-personas', version: 1, ...input, people: [input.people[0]] }),
  JSON.stringify({
    format: 'proteus-personas',
    version: 1,
    ...input,
    people: [{ ...input.people[0], context: 4 }, input.people[1]],
  }),
  JSON.stringify({
    format: 'proteus-personas',
    version: 1,
    ...input,
    people: [{ ...input.people[0], publicUrls: ['file:///etc/passwd'] }, input.people[1]],
  }),
  ' '.repeat(131073),
])('rejects malformed, incompatible or oversized presets', (data) => {
  expect(() => parsePersonaPreset(data)).toThrow();
});

it('normalizes legacy input into canonical fields while retaining the native v1 writer until IPC switches', () => {
  const input = {
    task: '  Общая задача\n',
    people: [
      { label: 'Анна', context: '  Борис: привет\n👩🏽‍💻  ', publicUrls: ['https://example.org/a-b'] },
      { label: 'Борис', context: 'Ответ Анне', publicUrls: [] },
    ] as const,
  };
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
  expect(parsePersonaPreset(JSON.stringify(legacy))).toEqual(input);
  expect(JSON.parse(serializePersonaPreset(input))).toEqual(legacy);
});
