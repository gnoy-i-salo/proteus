import { expect, it } from 'vitest';
import { parsePersonaPreset, serializePersonaPreset } from './index';
const input = {
  task: '',
  people: [
    {
      label: 'Анна',
      context: 'Точный контекст\nс переносом ✨',
      'public-urls': ['https://example.org/a'],
    },
    { label: 'Борис', context: 'Другой подход', 'public-urls': [] },
  ],
} as const;
it('round trips two persona inputs, Unicode and optional task without settings or results', () => {
  const data = serializePersonaPreset(input);
  expect(JSON.parse(data)).toEqual({ format: 'proteus-personas', version: 1, ...input });
  expect(parsePersonaPreset(data)).toEqual(input);
});
it.each([
  'not JSON',
  '{}',
  JSON.stringify({ format: 'proteus-personas', version: 2, ...input }),
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
    people: [{ ...input.people[0], 'public-urls': ['file:///etc/passwd'] }, input.people[1]],
  }),
  ' '.repeat(131073),
])('rejects malformed, incompatible or oversized presets', (data) => {
  expect(() => parsePersonaPreset(data)).toThrow();
});
