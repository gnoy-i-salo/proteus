import type { PersonaStyleProfile, StyleExemplar, StyleRule } from '@proteus-int/contracts';
import { decodeResponseFields, text, texts } from '@proteus-int/intelligence';

export function parseStyle(raw: unknown, samples: readonly StyleExemplar[]): PersonaStyleProfile {
  const value = decodeResponseFields(raw, [
    ['reasoning-patterns', 'reasoningPatterns'],
    ['voice-rules', 'voiceRules'],
    ['format-rules', 'formatRules'],
  ]);
  const known = new Set(samples.map((sample) => sample.id));
  const ids = new Set<string>();
  function rules(rawRules: unknown, reasoning = false): StyleRule[] {
    if (!Array.isArray(rawRules) || rawRules.length > 10)
      throw new Error('style: expected at most 10 rules');
    return rawRules.map((rawRule) => {
      const rule = decodeResponseFields(rawRule, [['exemplar-ids', 'exemplarIds']]);
      const id = text(rule.id, 'style.id');
      if (!/^[a-z]+-\d+$/u.test(id) || ids.has(id))
        throw new Error('style.id: expected unique opaque ID');
      ids.add(id);
      const references = texts(rule.exemplarIds, 'style.exemplarIds');
      if (
        !references.length ||
        new Set(references).size !== references.length ||
        references.some((id) => !known.has(id))
      )
        throw new Error('style.exemplarIds: unknown or missing exemplar');
      if (rule.status !== 'observed' && rule.status !== 'hypothesis')
        throw new Error('style.status: invalid');
      return {
        id,
        instruction: text(rule.instruction, 'style.instruction'),
        exemplarIds: references,
        status: reasoning ? 'hypothesis' : rule.status,
        limitations: [
          ...texts(rule.limitations, 'style.limitations'),
          ...(reasoning ? ['Это гипотеза по тексту, а не доступ к мыслям человека.'] : []),
        ],
      };
    });
  }
  return {
    version: 2,
    reasoningPatterns: rules(value.reasoningPatterns, true),
    voiceRules: rules(value.voiceRules),
    formatRules: rules(value.formatRules),
    contextModes: (['dialogue', 'comment', 'formal'] as const).map((mode) => ({
      mode,
      status: samples.some((sample) => sample.mode === mode) ? 'observed' : 'insufficient',
      limitations: samples.some((sample) => sample.mode === mode)
        ? []
        : ['Нет собственных образцов для этого контекста.'],
    })),
    styleExemplarIds: samples.map((sample) => sample.id),
    limitations: [
      ...texts(value.limitations, 'style.limitations'),
      'Небольшая выборка позволяет имитировать наблюдаемую манеру, но не гарантирует совпадение с человеком.',
    ],
  };
}
