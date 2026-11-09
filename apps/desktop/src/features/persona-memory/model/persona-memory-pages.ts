import type { PersonaRule, StyleRule } from '@proteus-int/contracts';
import {
  describeEvidenceStatus,
  describeSkillSupport,
  exemplarEvidenceTarget,
  skillEvidenceTargets,
} from './persona-memory-view';
import type { PersonaMemoryEvidenceTarget, PersonaMemoryViewModel } from './persona-memory-view';

export const profileSections = [
  ['profile', 'Профиль'],
  ['rules', 'Базовые правила'],
  ['adaptation', 'Адаптация к задаче'],
  ['memory', 'Память'],
  ['skills', 'Процедурные навыки'],
  ['style', 'Речь'],
  ['examples', 'Образцы речи'],
] as const;
export type ProfileSection = (typeof profileSections)[number][0];
export interface ProfilePage {
  readonly id: string;
  readonly label: string;
  readonly text: string;
  readonly evidence: readonly PersonaMemoryEvidenceTarget[];
}
const modes = { dialogue: 'Диалог', comment: 'Комментарий', formal: 'Формальный' } as const;
const limitations = (values: readonly string[]) =>
  values.length ? `Ограничения:\n${values.join('\n')}` : 'Ограничения не указаны.';
const page = (
  id: string,
  label: string,
  text: string,
  evidence: readonly PersonaMemoryEvidenceTarget[] = [],
): ProfilePage => ({ id, label, text, evidence });
function rulePages(rules: readonly PersonaRule[]): ProfilePage[] {
  return rules.map((rule) =>
    page(
      rule.id,
      rule.id,
      [
        rule.instruction,
        describeEvidenceStatus(rule.status),
        `Основания: ${rule.evidenceIds.join(', ') || 'не указаны'}`,
        limitations(rule.limitations),
      ].join('\n\n'),
    ),
  );
}
function styleRules(label: string, rules: readonly StyleRule[]): ProfilePage[] {
  return rules.map((rule) =>
    page(
      `${label}:${rule.id}`,
      `${label} · ${rule.id}`,
      [
        rule.instruction,
        rule.status === 'observed' ? 'Наблюдаемо' : 'Гипотеза',
        `Образцы: ${rule.exemplarIds.join(', ') || 'не указаны'}`,
        limitations(rule.limitations),
      ].join('\n\n'),
    ),
  );
}

export function createPersonaMemoryPages(model: PersonaMemoryViewModel) {
  const sections: Record<ProfileSection, () => readonly ProfilePage[]> = {
    profile: () => [
      page(
        'profile',
        'Профиль',
        [
          model.label(),
          model.sourceStatusLabel(),
          model.archetype(),
          ...model.profileSkills(),
          'Это поведенческая симуляция, не биография. Свидетельство источника само по себе не подтверждает истинность утверждения. Базовые правила и адаптация к задаче показаны отдельно.',
        ].join('\n\n'),
      ),
    ],
    rules: () => rulePages(model.baseRules()),
    adaptation: () =>
      model.adaptationError()
        ? [page('invalid', 'Адаптация недоступна', model.adaptationError()!)]
        : [
            ...(model.adaptationTask()
              ? [page('task', 'Задача адаптации', model.adaptationTask()!)]
              : []),
            ...rulePages(model.adaptationRules()),
          ],
    memory: () => {
      const memory = model.memorySummary();
      return [
        page(
          'memory',
          'Сохранённые ссылки',
          memory
            ? [
                `Манифест памяти: ${memory.manifestId}`,
                `Снимок памяти: ${memory.memorySnapshotId}`,
                `Версия схемы ссылки: ${memory.schemaVersion}`,
                'Это сохранённые ссылки. Доступность исходников и разрешения сейчас не проверены.',
              ].join('\n\n')
            : 'Ссылка на память в профиле не сохранена.',
        ),
      ];
    },
    skills: () =>
      model
        .proceduralSkills()
        .map((skill) =>
          page(
            skill.id,
            skill.id,
            [
              `Условие применения:\n${skill.trigger}`,
              `Входные данные:\n${skill.inputs.join('\n') || 'не указаны'}`,
              `Шаги:\n${skill.steps.map((step, i) => `${i + 1}. ${step}`).join('\n')}`,
              `Проверка результата:\n${skill.outcomeCheck}`,
              describeSkillSupport(skill.support),
              limitations(skill.limitations),
              `Элементы памяти: ${skill.memoryItemIds.join(', ') || 'не указаны'}`,
              ...skill.sourceSpans.map(
                (span) =>
                  `Документ: ${span.documentId}\nРевизия: ${span.revisionId}\nДиапазон: ${span.start}–${span.end} (${span.unit})`,
              ),
              'Наблюдение или предложение процедуры не доказывает способность модели выполнить её.',
            ].join('\n\n'),
            skillEvidenceTargets(skill),
          ),
        ),
    style: () => {
      const style = model.styleProfile();
      if (!style) return [];
      return [
        page(
          'style',
          'Область и ограничения',
          [
            `Версия схемы стиля: ${style.version}`,
            limitations(style.limitations),
            ...style.contextModes.map(
              (context) =>
                `${modes[context.mode]}: ${context.status === 'observed' ? 'наблюдаемо' : 'недостаточно данных'}\n${limitations(context.limitations)}`,
            ),
          ].join('\n\n'),
        ),
        ...styleRules('Рассуждения', style.reasoningPatterns),
        ...styleRules('Голос', style.voiceRules),
        ...styleRules('Оформление', style.formatRules),
      ];
    },
    examples: () =>
      model.styleExemplars().flatMap((exemplar) => {
        const target = exemplarEvidenceTarget(exemplar);
        const evidence = target ? [target] : [];
        return [
          page(`text:${exemplar.id}`, `${exemplar.id} · точный текст`, exemplar.text, evidence),
          page(
            `context:${exemplar.id}`,
            `${exemplar.id} · происхождение`,
            [
              exemplar.context,
              modes[exemplar.mode],
              `Документ: ${exemplar.documentId || 'не указан'}`,
              `Экспорт образца ${exemplar.exportAllowed ? 'разрешён при сохранении' : 'не разрешён'}. Текущее разрешение не проверено.`,
              ...(exemplar.redactions.length
                ? [
                    `В историческом образце отмечены замены: ${exemplar.redactions.map((span) => `${span.start}–${span.end}`).join(', ')}`,
                  ]
                : []),
            ].join('\n\n'),
            evidence,
          ),
        ];
      }),
  };
  const empty: Record<ProfileSection, string> = {
    profile: 'Профиль не сохранён.',
    rules: 'Базовые правила не сохранены.',
    adaptation: 'Адаптация к задаче не сохранена.',
    memory: 'Ссылка на память в профиле не сохранена.',
    skills: 'Процедурные навыки не сохранены.',
    style: 'Стилевой профиль не сохранён.',
    examples: 'Образцы речи не сохранены.',
  };
  return {
    items: (section: ProfileSection) => {
      const items = sections[section]();
      return items.length ? items : [page('empty', 'Нет данных', empty[section])];
    },
  };
}

export function evidenceTargetLabel(target: PersonaMemoryEvidenceTarget): string {
  if (target.kind === 'memoryItem') return `Элемент памяти ${target.memoryItemId}`;
  if (target.kind === 'sourceDocument')
    return `Документ ${target.documentId} · ${target.revisionId}`;
  return `Источник образца ${target.exemplarId}`;
}
