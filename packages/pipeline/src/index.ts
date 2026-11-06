import type { ExperimentInput } from '@proteus-int/contracts';
import { validateExperiment } from './validation';
export { validateExperiment } from './validation';
export * from './experiment';
export { exportExperiment } from './export';

export function parsePersonaPreset(data: string): ExperimentInput {
  if (new TextEncoder().encode(data).length > 131072) throw new Error('Пресет превышает 128 КиБ.');
  let value: unknown;
  try {
    value = JSON.parse(data);
  } catch {
    throw new Error('Файл .proteus содержит неверный JSON.');
  }
  if (!value || typeof value !== 'object') throw new Error('Неверный формат пресета.');
  const preset = value as Record<string, unknown>;
  if (preset['format'] !== 'proteus-personas' || preset['version'] !== 1)
    throw new Error('Неподдерживаемый формат или версия .proteus.');
  if (
    typeof preset['task'] !== 'string' ||
    !Array.isArray(preset['people']) ||
    preset['people'].length !== 2
  )
    throw new Error('Пресет должен содержать задачу и две персоны.');
  const people = preset['people'].map((item: unknown) => {
    if (!item || typeof item !== 'object') throw new Error('Неверная персона в пресете.');
    const person = item as Record<string, unknown>;
    if (
      typeof person['label'] !== 'string' ||
      typeof person['context'] !== 'string' ||
      !Array.isArray(person['public-urls']) ||
      !person['public-urls'].every((url: unknown) => typeof url === 'string')
    )
      throw new Error('Неверные поля персоны в пресете.');
    return {
      label: person['label'],
      context: person['context'],
      'public-urls': person['public-urls'] as string[],
    };
  });
  const input: ExperimentInput = { task: preset['task'], people: [people[0]!, people[1]!] };
  validateExperiment({ ...input, task: input.task.trim() ? input.task : 'Preset' });
  if (input.task.length > 4000) throw new Error('Задача превышает 4000 символов.');
  return input;
}
export function serializePersonaPreset(input: ExperimentInput): string {
  const clean = parsePersonaPreset(
    JSON.stringify({ format: 'proteus-personas', version: 1, ...input }),
  );
  return JSON.stringify({ format: 'proteus-personas', version: 1, ...clean }, null, 2);
}
