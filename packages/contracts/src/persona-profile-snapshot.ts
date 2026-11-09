import type { PersonaRule, PaletteId } from './persona';
import type { PersonaStyleProfile, StyleExemplar, StyleRule } from './style';
import { fields, choice } from './corpus-wire-validation';
import { memoryText, memoryId, memoryIds, memoryList } from './memory-wire-validation';

/** Task-free body; native assigns persona/manifest IDs and freezes skills separately. */
export interface PersonaProfileSnapshot {
  readonly schemaVersion: 1;
  readonly label: string;
  readonly palette: PaletteId;
  readonly archetype: string;
  readonly rules: readonly PersonaRule[];
  readonly sourceStatus: 'evidence-informed' | 'synthetic';
  readonly styleProfile: PersonaStyleProfile;
  readonly styleExemplars: readonly StyleExemplar[];
}
const maximumBytes = 524288;
const modes = ['dialogue', 'comment', 'formal'] as const;
function strings(value: unknown): string[] {
  return memoryList(value, maximumBytes / 2).map((item) => memoryText(item, maximumBytes));
}
function unique<T extends { readonly id: string }>(items: T[]): T[] {
  if (new Set(items.map((item) => item.id)).size !== items.length)
    throw new Error('Повторный идентификатор профиля.');
  return items;
}
function rule(value: unknown): PersonaRule {
  const input = fields(value, ['id', 'instruction', 'evidenceIds', 'status', 'limitations']);
  const evidenceIds = memoryIds(input.evidenceIds);
  if (!evidenceIds.length) throw new Error('Правило профиля требует основания.');
  return {
    id: memoryId(input.id),
    instruction: memoryText(input.instruction, maximumBytes),
    evidenceIds,
    status: choice(input.status, ['supported', 'hypothesis']),
    limitations: strings(input.limitations),
  };
}
function exemplar(value: unknown): StyleExemplar {
  const input = fields(value, [
    'id',
    'documentId',
    'mode',
    'text',
    'context',
    'redactions',
    'exportAllowed',
  ]);
  if (memoryList(input.redactions, 0).length || input.exportAllowed !== false)
    throw new Error('Подготовка не выдаёт разрешение на экспорт и не изменяет образцы.');
  return {
    id: memoryId(input.id),
    documentId: memoryId(input.documentId),
    mode: choice(input.mode, modes),
    text: memoryText(input.text, maximumBytes),
    context: memoryText(input.context, maximumBytes),
    redactions: [],
    exportAllowed: false,
  };
}
function style(value: unknown, samples: readonly StyleExemplar[]): PersonaStyleProfile {
  const input = fields(value, [
    'version',
    'reasoningPatterns',
    'voiceRules',
    'formatRules',
    'contextModes',
    'styleExemplarIds',
    'limitations',
  ]);
  if (input.version !== 2) throw new Error('Неизвестная версия стиля.');
  const ids = samples.map((sample) => sample.id);
  const known = new Set(ids);
  const readRules = (value: unknown, reasoning = false): StyleRule[] =>
    memoryList(value, 10).map((value) => {
      const input = fields(value, ['id', 'instruction', 'exemplarIds', 'status', 'limitations']);
      const exemplarIds = memoryIds(input.exemplarIds);
      if (!exemplarIds.length || exemplarIds.some((id) => !known.has(id)))
        throw new Error('Нет исходного образца правила стиля.');
      const status = choice(input.status, ['observed', 'hypothesis']);
      if (reasoning && status !== 'hypothesis') throw new Error('Ход мысли остаётся гипотезой.');
      return {
        id: memoryId(input.id),
        instruction: memoryText(input.instruction, maximumBytes),
        exemplarIds,
        status,
        limitations: strings(input.limitations),
      };
    });
  const reasoningPatterns = readRules(input.reasoningPatterns, true);
  const voiceRules = readRules(input.voiceRules);
  const formatRules = readRules(input.formatRules);
  unique([...reasoningPatterns, ...voiceRules, ...formatRules]);
  const contextModes = memoryList(input.contextModes, 3, 3).map((value) => {
    const input = fields(value, ['mode', 'status', 'limitations']);
    const mode = choice(input.mode, modes);
    const status = choice(input.status, ['observed', 'insufficient']);
    if ((status === 'observed') !== samples.some((sample) => sample.mode === mode))
      throw new Error('Режим стиля не соответствует образцам.');
    return { mode, status, limitations: strings(input.limitations) };
  });
  if (new Set(contextModes.map((item) => item.mode)).size !== 3)
    throw new Error('Повторный режим стиля.');
  const styleExemplarIds = memoryIds(input.styleExemplarIds);
  if (JSON.stringify(ids) !== JSON.stringify(styleExemplarIds))
    throw new Error('Состав образцов стиля изменён.');
  return {
    version: 2,
    reasoningPatterns,
    voiceRules,
    formatRules,
    contextModes,
    styleExemplarIds,
    limitations: strings(input.limitations),
  };
}
/** Strict new preparation schema; historical profiles use their own era decoders. */
export function decodePersonaProfileSnapshot(value: unknown): PersonaProfileSnapshot {
  memoryText(JSON.stringify(value), maximumBytes);
  const input = fields(value, [
    'schemaVersion',
    'label',
    'palette',
    'archetype',
    'rules',
    'sourceStatus',
    'styleProfile',
    'styleExemplars',
  ]);
  if (input.schemaVersion !== 1) throw new Error('Неизвестная версия тела профиля.');
  const styleExemplars = unique(memoryList(input.styleExemplars, 100).map(exemplar));
  const rules = unique(memoryList(input.rules, 10).map(rule));
  const sourceStatus = choice(input.sourceStatus, ['evidence-informed', 'synthetic']);
  if ((sourceStatus === 'evidence-informed') !== rules.some((rule) => rule.status === 'supported'))
    throw new Error('Статус профиля не соответствует основаниям.');
  return {
    schemaVersion: 1,
    label: memoryText(input.label, 512),
    palette: choice(input.palette, ['turquoise', 'violet']),
    archetype: memoryText(input.archetype, maximumBytes),
    rules,
    sourceStatus,
    styleProfile: style(input.styleProfile, styleExemplars),
    styleExemplars,
  };
}
