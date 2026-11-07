import type { ProviderId } from '@proteus-int/contracts';
import { componentIds, isProviderId, type ComponentId } from './model-settings';

// Boundary shape prepared before the atomic domain/consumer migration.
export interface StoredModelSettings {
  readonly globalModel: ProviderId;
  readonly components: Readonly<Record<ComponentId, ProviderId>>;
  readonly aliases: Readonly<Record<ComponentId, string>>;
}

function record(value: unknown): Record<string, unknown> {
  if (!value || typeof value !== 'object' || Array.isArray(value))
    throw new Error('Неверные настройки моделей.');
  return value as Record<string, unknown>;
}

function keys(value: Record<string, unknown>, allowed: readonly string[]): void {
  if (Object.keys(value).some((key) => !allowed.includes(key)))
    throw new Error('Неизвестные или смешанные поля настроек моделей.');
}

function componentMap<T>(value: unknown, parse: (item: unknown) => T): Record<ComponentId, T> {
  const source = record(value);
  keys(source, componentIds);
  return {
    stalker: parse(source['stalker']),
    'person-fabric': parse(source['person-fabric']),
    person: parse(source['person']),
    'brain-analyzator': parse(source['brain-analyzator']),
  };
}

function provider(value: unknown): ProviderId {
  if (!isProviderId(value)) throw new Error('Неизвестный провайдер модели.');
  return value;
}

function alias(value: unknown): string {
  if (typeof value !== 'string' || value.length > 120)
    throw new Error('Неверный псевдоним модели.');
  return value;
}

export function decodeModelSettings(data: string): StoredModelSettings {
  if (new TextEncoder().encode(data).length > 16384)
    throw new Error('Настройки моделей превышают 16 КиБ.');
  const value = record(JSON.parse(data));
  const legacy = !('format' in value) && !('version' in value);
  if (legacy) keys(value, ['global-model', 'components', 'aliases']);
  else {
    keys(value, ['format', 'version', 'globalModel', 'components', 'aliases']);
    if (value['format'] !== 'proteus-model-settings' || value['version'] !== 2)
      throw new Error('Неподдерживаемая версия настроек моделей.');
  }
  return {
    globalModel: provider(value[legacy ? 'global-model' : 'globalModel']),
    components: componentMap(value['components'], provider),
    aliases:
      legacy && !('aliases' in value)
        ? { stalker: '', 'person-fabric': '', person: '', 'brain-analyzator': '' }
        : componentMap(value['aliases'], alias),
  };
}

export function encodeModelSettings(settings: StoredModelSettings): string {
  const data = JSON.stringify({ format: 'proteus-model-settings', version: 2, ...settings });
  decodeModelSettings(data);
  return data;
}
