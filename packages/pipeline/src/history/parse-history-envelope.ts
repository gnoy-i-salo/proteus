export interface HistoryEnvelope {
  readonly formatVersion: 1 | 2 | 3 | 4;
  readonly value: Readonly<Record<string, unknown>>;
}

function parseObject(text: string): Record<string, unknown> {
  const maxBytes = 8 * 1024 * 1024;
  if (text.length > maxBytes) throw new Error('Файл результата превышает 8 МиБ.');
  const bytes = new TextEncoder().encode(text);
  if (bytes.length > maxBytes) throw new Error('Файл результата превышает 8 МиБ.');
  if (new TextDecoder().decode(bytes) !== text)
    throw new Error('Файл результата содержит неверный UTF-8.');
  const parsed: unknown = JSON.parse(text);
  if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed))
    throw new Error('Файл результата должен содержать объект JSON.');
  return parsed as Record<string, unknown>;
}

/** Routes known envelopes only; the owning era decoder must validate the body. */
export function parseHistoryEnvelope(text: string): HistoryEnvelope {
  const value = parseObject(text);
  const owns = (key: string) => Object.hasOwn(value, key);
  const camel = owns('schemaVersion');
  const legacy = owns('schema-version');
  if (camel && legacy) throw new Error('Смешанные версии формата результата.');
  const version = camel ? value.schemaVersion : legacy ? value['schema-version'] : 1;
  if ((camel && version !== 4) || (legacy && version !== 2 && version !== 3))
    throw new Error('Неподдерживаемая версия результата.');
  const formatVersion = version as HistoryEnvelope['formatVersion'];
  const required =
    formatVersion === 1
      ? ['input', 'people', 'comparison', 'finished-at']
      : [
          camel ? 'schemaVersion' : 'schema-version',
          camel ? 'scoringVersion' : 'scoring-version',
          'status',
          'input',
          'protocol',
          'people',
          'control',
          'trials',
          'comparison',
          'traces',
          camel ? 'finishedAt' : 'finished-at',
        ];
  const optional =
    formatVersion === 4
      ? ['execution', 'exportMetadata']
      : formatVersion === 3
        ? ['export-metadata']
        : [];
  if (
    required.some((key) => !owns(key)) ||
    Object.keys(value).some((key) => !required.includes(key) && !optional.includes(key))
  )
    throw new Error('Неизвестные, отсутствующие или смешанные поля результата.');
  if (formatVersion !== 1 && value[camel ? 'scoringVersion' : 'scoring-version'] !== 2)
    throw new Error('Неподдерживаемая версия оценки результата.');
  return { formatVersion, value };
}
