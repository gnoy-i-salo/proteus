import { decodeModelSettings, encodeModelSettings } from '@proteus-int/intelligence';
import type { StoredModelSettings } from '@proteus-int/intelligence';

export interface ModelSettingsStorage {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
  removeItem(key: string): void;
}

export const modelSettingsKeys = {
  legacy: 'proteus-model-settings',
  current: 'proteus-model-settings-v2',
  pending: 'proteus-model-settings-v2-pending',
  legacySnapshot: 'proteus-model-settings-v2-legacy-snapshot',
  recovery: 'proteus-model-settings-v2-recovery',
} as const;

function clearPending(storage: ModelSettingsStorage): boolean {
  try {
    for (const key of [modelSettingsKeys.pending, modelSettingsKeys.legacySnapshot])
      if (storage.getItem(key) !== null) storage.removeItem(key);
    return false;
  } catch {
    // A verified current value remains usable even when staging cleanup fails.
    return true;
  }
}

function decodeCurrent(data: string): StoredModelSettings {
  // Apply the codec's byte limit before any JSON parse at this boundary.
  const settings = decodeModelSettings(data);
  const envelope: unknown = JSON.parse(data);
  if (
    !envelope ||
    typeof envelope !== 'object' ||
    !('version' in envelope) ||
    envelope.version !== 2
  )
    throw new Error('Неподдерживаемая сохранённая версия настроек моделей.');
  return settings;
}

function canRecoverMalformed(data: string): boolean {
  if (new TextEncoder().encode(data).length > 16 * 1024) return false;
  let envelope: unknown;
  try {
    envelope = JSON.parse(data);
  } catch {
    return true;
  }
  if (!envelope || typeof envelope !== 'object') return true;
  // A future or foreign envelope may be valid for another writer; preserve it unchanged.
  if ('version' in envelope && envelope.version !== 2) return false;
  if ('format' in envelope && envelope.format !== 'proteus-model-settings') return false;
  return true;
}

function quarantinePublication(storage: ModelSettingsStorage, malformed: string): boolean {
  if (!canRecoverMalformed(malformed)) return false;
  const original = storage.getItem(modelSettingsKeys.legacySnapshot);
  const pending = storage.getItem(modelSettingsKeys.pending);
  if (
    original === null ||
    pending === null ||
    storage.getItem(modelSettingsKeys.legacy) !== original
  )
    return false;
  const encoded = encodeModelSettings(decodeModelSettings(original));
  if (pending !== encoded || storage.getItem(modelSettingsKeys.current) !== malformed) return false;
  const recovered = storage.getItem(modelSettingsKeys.recovery);
  if (recovered !== null && recovered !== malformed)
    throw new Error(
      'Сохранённая копия восстановления требует проверки; исходные данные не изменены.',
    );
  if (recovered === null) storage.setItem(modelSettingsKeys.recovery, malformed);
  if (storage.getItem(modelSettingsKeys.recovery) !== malformed)
    throw new Error('Не удалось проверить копию повреждённых настроек.');
  if (
    storage.getItem(modelSettingsKeys.current) !== malformed ||
    storage.getItem(modelSettingsKeys.legacy) !== original ||
    storage.getItem(modelSettingsKeys.legacySnapshot) !== original ||
    storage.getItem(modelSettingsKeys.pending) !== encoded
  )
    throw new Error('Настройки изменились во время восстановления; повторите проверку.');
  storage.removeItem(modelSettingsKeys.current);
  if (storage.getItem(modelSettingsKeys.current) !== null)
    throw new Error('Настройки изменились во время восстановления; повторите проверку.');
  return true;
}

function rejectPublication(storage: ModelSettingsStorage, published: string | null): never {
  if (published !== null) {
    let malformed = false;
    try {
      decodeCurrent(published);
    } catch {
      malformed = true;
    }
    // A different valid v2 value may belong to another writer and is never rolled back.
    if (malformed) quarantinePublication(storage, published);
  }
  throw new Error('Не удалось проверить сохранённые настройки моделей.');
}

export function migrateModelSettings(storage: ModelSettingsStorage): {
  settings: StoredModelSettings;
  migrated: boolean;
  cleanupPending: boolean;
} | null {
  const current = storage.getItem(modelSettingsKeys.current);
  if (current !== null) {
    let settings: StoredModelSettings | undefined;
    try {
      settings = decodeCurrent(current);
    } catch (error) {
      if (!quarantinePublication(storage, current)) throw error;
    }
    if (settings !== undefined)
      return { settings, migrated: false, cleanupPending: clearPending(storage) };
  }
  const raw = storage.getItem(modelSettingsKeys.legacy);
  if (raw === null) return null;
  const settings = decodeModelSettings(raw);
  const encoded = encodeModelSettings(settings);
  // Retain exact legacy bytes and a verified candidate so restart can identify our publication.
  // localStorage guarantees individual writes, not a transaction or compare-and-swap across keys.
  storage.setItem(modelSettingsKeys.legacySnapshot, raw);
  if (storage.getItem(modelSettingsKeys.legacySnapshot) !== raw)
    throw new Error('Не удалось проверить исходные настройки моделей.');
  storage.setItem(modelSettingsKeys.pending, encoded);
  if (storage.getItem(modelSettingsKeys.pending) !== encoded)
    throw new Error('Не удалось проверить подготовленные настройки моделей.');
  if (storage.getItem(modelSettingsKeys.legacy) !== raw)
    throw new Error('Исходные настройки изменились; миграция не опубликована.');
  if (storage.getItem(modelSettingsKeys.current) !== null) {
    clearPending(storage);
    throw new Error('Настройки уже записаны другим вызовом; миграция не опубликована.');
  }
  storage.setItem(modelSettingsKeys.current, encoded);
  const published = storage.getItem(modelSettingsKeys.current);
  if (published !== encoded) rejectPublication(storage, published);
  return { settings, migrated: true, cleanupPending: clearPending(storage) };
}
