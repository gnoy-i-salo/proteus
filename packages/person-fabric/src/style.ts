import type { PersonaStyleProfile, StyleExemplar, StyleRule } from '@proteus-int/contracts';
import { object, text, texts } from '@proteus-int/intelligence';

export function parseStyle(raw: unknown, samples: readonly StyleExemplar[]): PersonaStyleProfile {
  const value = object(raw);
  const known = new Set(samples.map((sample) => sample.id));
  const ids = new Set<string>();
  function rules(rawRules: unknown, reasoning = false): StyleRule[] {
    if (!Array.isArray(rawRules) || rawRules.length > 10)
      throw new Error('style: expected at most 10 rules');
    return rawRules.map((rawRule) => {
      const rule = object(rawRule);
      const id = text(rule.id, 'style.id');
      if (!/^[a-z]+-\d+$/u.test(id) || ids.has(id))
        throw new Error('style.id: expected unique opaque ID');
      ids.add(id);
      const references = texts(rule['exemplar-ids'], 'style.exemplar-ids');
      if (
        !references.length ||
        new Set(references).size !== references.length ||
        references.some((id) => !known.has(id))
      )
        throw new Error('style.exemplar-ids: unknown or missing exemplar');
      if (rule.status !== 'observed' && rule.status !== 'hypothesis')
        throw new Error('style.status: invalid');
      return {
        id,
        instruction: text(rule.instruction, 'style.instruction'),
        'exemplar-ids': references,
        status: reasoning ? 'hypothesis' : rule.status,
        limitations: [
          ...texts(rule.limitations, 'style.limitations'),
          ...(reasoning ? ['Это гипотеза по тексту, а не доступ к мыслям человека.'] : []),
        ],
      };
    });
  }
  return {
    version: 1,
    'reasoning-patterns': rules(value['reasoning-patterns'], true),
    'voice-rules': rules(value['voice-rules']),
    'format-rules': rules(value['format-rules']),
    'context-modes': (['dialogue', 'comment', 'formal'] as const).map((mode) => ({
      mode,
      status: samples.some((sample) => sample.mode === mode) ? 'observed' : 'insufficient',
      limitations: samples.some((sample) => sample.mode === mode)
        ? []
        : ['Нет собственных образцов для этого контекста.'],
    })),
    'style-exemplar-ids': samples.map((sample) => sample.id),
    limitations: [
      ...texts(value.limitations, 'style.limitations'),
      'Небольшая выборка позволяет имитировать наблюдаемую манеру, но не гарантирует совпадение с человеком.',
    ],
  };
}
