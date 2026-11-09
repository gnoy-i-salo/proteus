import spec from './persona-memory-prompt.json';
import type { PersonaProfileSnapshot } from './persona-profile-snapshot';

/** Renderer pinned by person-memory-profile:2; native uses the same checked-in text. */
export function personaMemoryPrompt(
  sourceStatus: PersonaProfileSnapshot['sourceStatus'],
  formatAttempt: 1 | 2,
  taskAdaptation = false,
): string {
  if (spec.schemaVersion !== 1) throw new Error('Неизвестная версия инструкций Person.');
  return (
    'PERSON memory\n' +
    (sourceStatus === 'neutral' ? spec.neutralPerspective : spec.personaPerspective) +
    spec.body +
    (formatAttempt === 2 ? spec.repair : '') +
    (taskAdaptation ? spec.taskGuidance : '')
  );
}
