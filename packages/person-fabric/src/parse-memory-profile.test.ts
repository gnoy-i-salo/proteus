import { expect, it } from 'vitest';
import type { FabricContextLease, MemoryItem } from '@proteus-int/contracts';
import { parseMemoryProfile } from './parse-memory-profile';
import { parseMemorySkills } from './parse-memory-skills';
import { lease, selection, source } from './testing/fabric-fixtures';
import { skillMemoryItem } from './testing/memory-skill-fixtures';

const original = '\uFEFF  Анна → Борис\r\n«Ёлка» é 👩🏽‍🔬\n\n';
function context(item: MemoryItem): FabricContextLease {
  return {
    ...lease({ schemaVersion: 1, ...selection, systemPrompt: '', task: '', formatAttempt: 1 }),
    items: [item],
    sources: [
      {
        ...source,
        text: item.text,
        span: item.sourceSpans[0]!,
        relations: [
          {
            kind: 'reply',
            original: { documentId: 'boris-doc', revisionId: 'boris-rev' },
            originalAuthor: { provider: 'telegram', id: 'boris', label: 'Борис' },
            span: null,
          },
        ],
        sourceMetadata: {
          schemaVersion: 1,
          sourceFormat: 'telegramJson:1',
          resourceLabel: 'Диалог',
          title: null,
          authorLabels: ['Анна', '@anna'],
          originalObjectId: 'original-1',
          limitations: [],
        },
      },
    ],
  };
}
const emptyStyle = { reasoningPatterns: [], voiceRules: [], formatRules: [], limitations: [] };
function proposed(item: MemoryItem) {
  return {
    draftId: 'draft:1',
    trigger: '  Начало\r\n',
    inputs: [],
    steps: ['  Шаг é\r\n'],
    outcomeCheck: 'Результат',
    support: 'proposed',
    memoryItemIds: [item.id],
    sourceSpans: item.sourceSpans,
    limitations: ['Предложение, не наблюдение'],
  };
}
it('keeps exact own samples, names and relationships independently from generated style rules', () => {
  const item = skillMemoryItem({
    kind: 'styleExample',
    text: original,
    limitations: ['Один образец'],
  });
  const result = parseMemoryProfile(
    {
      rules: [],
      skills: [],
      style: {
        ...emptyStyle,
        formatRules: [
          {
            id: 'format-1',
            instruction: 'Сохранить пустые строки',
            exemplarIds: [item.id],
            status: 'observed',
            limitations: [],
          },
        ],
      },
    },
    context(item),
    10,
  );
  expect(result.exemplars[0]?.text).toBe(original);
  expect(result.exemplars[0]?.exportAllowed).toBe(false);
  expect(JSON.parse(result.exemplars[0]!.context)).toMatchObject({
    document: { authorLabel: 'Анна', replyTo: 'boris-message' },
    relations: [{ originalAuthor: { label: 'Борис' } }],
    sourceMetadata: { authorLabels: ['Анна', '@anna'] },
  });
  expect(result.style.limitations).toContain('Один образец');
});
it('keeps reported assertions tentative, preserves source limitations and rejects unknown/conflicting rules', () => {
  const item = skillMemoryItem({
    support: 'reported',
    kind: 'reportedAssertion',
    limitations: ['Не проверено'],
  });
  const rule = {
    id: 'r1',
    instruction: 'Возможно проверяет',
    evidenceIds: [item.id],
    status: 'hypothesis',
    limitations: [],
  };
  const value = { rules: [rule], style: emptyStyle, skills: [proposed(item)] };
  const result = parseMemoryProfile(value, context(item), 10);
  expect(result.rules[0]?.limitations).toContain('Не проверено');
  expect(result.rules[0]?.limitations).toContain(
    'Это сообщение источника, а не независимо подтверждённый факт.',
  );
  expect(result.skills[0]?.support).toBe('proposed');
  expect(result.skills[0]?.limitations).toContain('Не проверено');
  expect(result.skills[0]?.steps).toEqual(proposed(item).steps);
  expect(() =>
    parseMemoryProfile({ ...value, rules: [{ ...rule, status: 'supported' }] }, context(item), 10),
  ).toThrow();
  for (const patch of [
    { support: 'unknown' as const },
    { support: 'conflicting' as const },
    { contradictionIds: ['other'] },
  ])
    expect(() => parseMemoryProfile(value, context({ ...item, ...patch }), 10)).toThrow();
});
it('refuses invented, foreign or unattributed exemplars and unknown response fields', () => {
  const item = skillMemoryItem({ kind: 'styleExample', text: original });
  const value = { rules: [], style: emptyStyle, skills: [] };
  const scoped = context(item);
  for (const patch of [
    { canSupportOwnWords: false },
    { text: 'rewritten source' },
    { span: { ...scoped.sources[0]!.span, revisionId: 'foreign' } },
  ])
    expect(() =>
      parseMemoryProfile(value, { ...scoped, sources: [{ ...scoped.sources[0]!, ...patch }] }, 10),
    ).toThrow();
  expect(() => parseMemoryProfile({ ...value, manifestId: 'model-owned' }, scoped, 10)).toThrow();
  expect(
    parseMemoryProfile(value, { ...scoped, items: [{ ...item, contradictionIds: ['other'] }] }, 10)
      .exemplars,
  ).toEqual([]);
});
it('accepts the native 64 contradiction references as scope without treating them as skill evidence', () => {
  const item = skillMemoryItem({
    contradictionIds: Array.from({ length: 64 }, (_, i) => 'other:' + i),
  });
  expect(parseMemorySkills({ skills: [proposed(item)] }, { items: [item], maxSkills: 10 })).toEqual(
    [proposed(item)],
  );
  expect(() =>
    parseMemorySkills(
      { skills: [{ ...proposed(item), support: 'observed' }] },
      { items: [item], maxSkills: 10 },
    ),
  ).toThrow(/contradiction/u);
  expect(() =>
    parseMemorySkills(
      { skills: [] },
      {
        items: [{ ...item, contradictionIds: [...item.contradictionIds, 'overflow'] }],
        maxSkills: 10,
      },
    ),
  ).toThrow();
});
it('counts contradiction IDs in the existing total scope reference byte budget', () => {
  const items = Array.from({ length: 300 }, (_, i) =>
    skillMemoryItem({
      id: 'item:' + i,
      contradictionIds: Array.from({ length: 16 }, (_, index) => String(index).padStart(256, 'x')),
    }),
  );
  expect(() => parseMemorySkills({ skills: [] }, { items, maxSkills: 10 })).toThrow(/1 MiB/u);
});
