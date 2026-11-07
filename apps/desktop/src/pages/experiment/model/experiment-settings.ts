import { createSignal } from 'solid-js';
import { createModelSettings, encodeModelSettings } from '@proteus-int/intelligence';
import type { ModelSettings } from '@proteus-int/intelligence';
import {
  migrateModelSettings,
  modelSettingsKeys,
} from '../../../shared/lib/model-settings-storage';
import type { ModelSettingsStorage } from '../../../shared/lib/model-settings-storage';

function readSettings(storage: () => ModelSettingsStorage) {
  try {
    const restored = migrateModelSettings(storage());
    return {
      settings: restored?.settings ?? createModelSettings(),
      error: restored?.cleanupPending
        ? 'Настройки восстановлены, но очистка временной записи не завершена.'
        : '',
    };
  } catch {
    return {
      settings: createModelSettings(),
      error:
        'Не удалось восстановить настройки моделей. Исходная запись сохранена; проверьте настройки.',
    };
  }
}
export function createExperimentSettings(
  options: {
    readonly clearSourceGrants: () => void;
    readonly reportError: (message: string) => void;
  },
  storage: () => ModelSettingsStorage = () => localStorage,
) {
  const restored = readSettings(storage);
  const [settings, setSettings] = createSignal(restored.settings);
  options.reportError(restored.error);
  const sourceModelsAvailable = () =>
    Object.values(settings().components).every((provider) => provider === 'codex');
  const persist = (value: ModelSettings) => {
    if (
      value.components.stalker !== settings().components.stalker ||
      value.aliases.stalker !== settings().aliases.stalker
    )
      options.clearSourceGrants();
    setSettings(value);
    try {
      storage().setItem(modelSettingsKeys.current, encodeModelSettings(value));
    } catch {
      options.reportError('Настройки применены, но не сохранены.');
    }
  };
  return { settings, sourceModelsAvailable, persist };
}
