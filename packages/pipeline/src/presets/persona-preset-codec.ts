import { validateExperiment } from '../validation';

export interface StoredPersonaInput {
  readonly label: string;
  readonly context: string;
  readonly publicUrls: readonly string[];
}

export interface StoredPersonaPreset {
  readonly task: string;
  readonly people: readonly [StoredPersonaInput, StoredPersonaInput];
}

function checkSize(data: string): void {
  if (new TextEncoder().encode(data).length > 131072) throw new Error('Пресет превышает 128 КиБ.');
}

function record(value: unknown): Record<string, unknown> {
  if (!value || typeof value !== 'object' || Array.isArray(value))
    throw new Error('Неверный формат пресета.');
  return value as Record<string, unknown>;
}

function keys(value: Record<string, unknown>, allowed: readonly string[]): void {
  if (Object.keys(value).some((key) => !allowed.includes(key)))
    throw new Error('Неизвестные или смешанные поля пресета.');
}

function person(value: unknown, legacy: boolean): StoredPersonaInput {
  const source = record(value);
  const urlsKey = legacy ? 'public-urls' : 'publicUrls';
  keys(source, ['label', 'context', urlsKey]);
  const urls = source[urlsKey];
  if (
    typeof source.label !== 'string' ||
    typeof source.context !== 'string' ||
    !Array.isArray(urls) ||
    !urls.every((url: unknown) => typeof url === 'string')
  )
    throw new Error('Неверные поля персоны в пресете.');
  return { label: source.label, context: source.context, publicUrls: [...urls] };
}

export function decodePersonaPreset(data: string): StoredPersonaPreset {
  checkSize(data);
  let value: unknown;
  try {
    value = JSON.parse(data);
  } catch {
    throw new Error('Файл .proteus содержит неверный JSON.');
  }
  const preset = record(value);
  keys(preset, ['format', 'version', 'task', 'people']);
  if (preset.format !== 'proteus-personas' || (preset.version !== 1 && preset.version !== 2))
    throw new Error('Неподдерживаемый формат или версия .proteus.');
  if (
    typeof preset.task !== 'string' ||
    !Array.isArray(preset.people) ||
    preset.people.length !== 2
  )
    throw new Error('Пресет должен содержать задачу и две персоны.');
  const people: StoredPersonaPreset['people'] = [
    person(preset.people[0], preset.version === 1),
    person(preset.people[1], preset.version === 1),
  ];
  // Reuse the current domain rules until the atomic consumer migration.
  const legacyPerson = (value: StoredPersonaInput) => ({
    label: value.label,
    context: value.context,
    'public-urls': value.publicUrls,
  });
  validateExperiment({
    task: preset.task.trim() ? preset.task : 'Preset',
    people: [legacyPerson(people[0]), legacyPerson(people[1])],
  });
  if (preset.task.length > 4000) throw new Error('Задача превышает 4000 символов.');
  return { task: preset.task, people };
}

export function encodePersonaPreset(input: StoredPersonaPreset): string {
  keys(record(input), ['task', 'people']);
  const clean = decodePersonaPreset(
    JSON.stringify({ format: 'proteus-personas', version: 2, ...input }),
  );
  const data = JSON.stringify({ format: 'proteus-personas', version: 2, ...clean }, null, 2);
  checkSize(data);
  return data;
}
