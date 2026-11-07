import type { PersonProfile } from '@proteus-int/contracts';

export function neutralProfile(): PersonProfile {
  return {
    id: 'neutral',
    label: 'Нейтральный контроль',
    palette: 'turquoise',
    archetype: 'Решайте общую задачу без приписанных персональных черт, привычек или способностей.',
    skills: [],
    rules: [],
    taskSkills: [],
    sourceStatus: 'neutral',
  };
}
