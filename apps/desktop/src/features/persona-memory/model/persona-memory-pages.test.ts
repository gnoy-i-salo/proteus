import { expect, it } from 'vitest';
import type { PersonProfile } from '@proteus-int/contracts';
import { createPersonaMemoryPages } from './persona-memory-pages';
import { createPersonaMemoryViewModel } from './persona-memory-view';

const profile: PersonProfile = {
  id: 'stable-person',
  label: 'Автор',
  palette: 'turquoise',
  archetype: 'База',
  skills: [],
  rules: [
    {
      id: 'r1',
      instruction: 'Правило',
      evidenceIds: ['e1'],
      status: 'conflicting',
      limitations: ['Не разрешено'],
    },
  ],
  taskSkills: [],
  sourceStatus: 'evidence-informed',
  styleExemplars: [
    {
      id: 'sample',
      documentId: 'doc',
      mode: 'dialogue',
      text: '  Ёж 👩🏽‍🔬\r\n\t<em>слова</em>  ',
      context: 'Имя автора и собеседник',
      redactions: [],
      exportAllowed: false,
    },
  ],
};

it('keeps exact sample text apart from its attribution, context and permission metadata', () => {
  const pages = createPersonaMemoryPages(createPersonaMemoryViewModel({ profile: () => profile }));
  const items = pages.items('examples');
  expect(items[0]?.text).toBe(profile.styleExemplars![0]!.text);
  expect(items[1]?.text).toContain('Имя автора и собеседник');
  expect(items[1]?.text).toContain('не разрешён');
  expect(items[0]?.evidence).toEqual([
    { kind: 'styleExemplar', exemplarId: 'sample', documentId: 'doc' },
  ]);
});

it('shows contradictory rule provenance and limitations without inventing memory', () => {
  const pages = createPersonaMemoryPages(createPersonaMemoryViewModel({ profile: () => profile }));
  expect(pages.items('rules')[0]?.text).toContain('Противоречие');
  expect(pages.items('rules')[0]?.text).toContain('Не разрешено');
  expect(pages.items('rules')[0]?.text).toContain('e1');
  expect(pages.items('memory')[0]?.text).toBe('Ссылка на память в профиле не сохранена.');
});
