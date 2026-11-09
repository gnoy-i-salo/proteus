import type { PersonaRule } from './persona';
import type { PersonaProfileSnapshot } from './persona-profile-snapshot';
import { fields } from './corpus-wire-validation';
import { memoryText, memoryId, memoryIds, memoryList } from './memory-wire-validation';

/** Per-task guidance, never part of the frozen task-free profile or source memory. */
export interface PersonaTaskAdaptation {
  readonly schemaVersion: 1;
  readonly manifestId: string;
  readonly task: string;
  readonly strategies: readonly PersonaRule[];
}

export function decodePersonaTaskAdaptation(
  value: unknown,
  scope: {
    readonly manifestId: string;
    readonly task: string;
    readonly profile: Pick<PersonaProfileSnapshot, 'rules' | 'sourceStatus'>;
  },
): PersonaTaskAdaptation {
  memoryText(JSON.stringify(value), 150000);
  const input = fields(value, ['schemaVersion', 'manifestId', 'task', 'strategies']);
  const manifestId = memoryId(input.manifestId);
  const task = memoryText(input.task, 100000);
  if (input.schemaVersion !== 1 || manifestId !== scope.manifestId || task !== scope.task)
    throw new Error('Стратегия относится к другому профилю или задаче.');
  const strategies = memoryList(input.strategies, 5, 1).map((value): PersonaRule => {
    const rule = fields(value, ['id', 'instruction', 'evidenceIds', 'status', 'limitations']);
    const id = memoryId(rule.id);
    const instruction = memoryText(rule.instruction, 96000);
    const evidenceIds = memoryIds(rule.evidenceIds);
    const limitations = memoryList(rule.limitations, 1000).map((value) =>
      memoryText(value, 150000),
    );
    if (!/^s\d+$/u.test(id) || !instruction.trim() || rule.status !== 'hypothesis')
      throw new Error('Адаптация к задаче остаётся предполагаемой стратегией.');
    if (scope.profile.sourceStatus === 'neutral' && evidenceIds.length)
      throw new Error('Нейтральная стратегия не ссылается на личность.');
    for (const evidenceId of evidenceIds) {
      const rules = scope.profile.rules.filter((rule) => rule.evidenceIds.includes(evidenceId));
      if (
        !rules.length ||
        rules.some((rule) => rule.status === 'unknown' || rule.status === 'conflicting')
      )
        throw new Error('Нет основания стратегии в закреплённом профиле.');
      if (
        rules.some((rule) =>
          rule.limitations.some((limitation) => !limitations.includes(limitation)),
        )
      )
        throw new Error('Стратегия потеряла ограничения исходных наблюдений.');
    }
    return { id, instruction, evidenceIds, status: 'hypothesis', limitations };
  });
  if (new Set(strategies.map((rule) => rule.id)).size !== strategies.length)
    throw new Error('Повторная стратегия задачи.');
  return { schemaVersion: 1, manifestId, task, strategies };
}
