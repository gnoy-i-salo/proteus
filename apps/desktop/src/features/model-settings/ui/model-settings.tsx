import { createSignal, For, Show } from 'solid-js';
import {
  componentIds,
  createModelSettings,
  isProviderId,
  parseModelSettings,
  setComponentModel,
  setGlobalModel,
} from '@proteus-int/intelligence';
import type { ModelSettings } from '@proteus-int/intelligence';
import styles from './model-settings.module.scss';
const storageKey = 'proteus-model-settings';
const labels = {
  stalker: 'STALKER',
  'person-fabric': 'Person Fabric',
  person: 'Person',
  'brain-analyzator': 'Brain Analyzator',
};
function readSettings(): ModelSettings {
  try {
    return parseModelSettings(localStorage.getItem(storageKey));
  } catch {
    return createModelSettings();
  }
}
export function ModelSettingsPanel() {
  const [settings, setSettings] = createSignal(readSettings());
  const [error, setError] = createSignal('');
  const persist = (value: ModelSettings) => {
    setSettings(value);
    try {
      localStorage.setItem(storageKey, JSON.stringify(value));
      setError('');
    } catch {
      setError('Настройки применены, но не сохранятся после перезапуска.');
    }
  };
  return (
    <section class={styles['model-settings']} aria-labelledby="settings-title">
      <div class={styles['model-settings__heading']}>
        <span>02 / CONFIGURATION</span>
        <h2 id="settings-title">Настройки PROTEUS</h2>
        <p>Выберите модель для каждого компонента.</p>
      </div>
      <div class={styles['model-settings__global']}>
        <label for="global-model">
          Общая модель<small>Применяется сразу ко всем компонентам</small>
        </label>
        <select
          id="global-model"
          aria-label="Общая модель"
          value={settings()['global-model']}
          onChange={(event) => {
            const value = event.currentTarget.value;
            if (isProviderId(value)) persist(setGlobalModel(settings(), value));
          }}
        >
          <option value="codex">Codex</option>
          <option value="kimi">Kimi</option>
        </select>
        <button
          type="button"
          onClick={() => persist(setGlobalModel(settings(), settings()['global-model']))}
        >
          Применить ко всем
        </button>
      </div>
      <div class={styles['model-settings__components']}>
        <For each={componentIds}>
          {(id) => (
            <label class={styles['model-settings__row']}>
              <span>{labels[id]}</span>
              <select
                aria-label={`Модель ${labels[id]}`}
                value={settings().components[id]}
                onChange={(event) => {
                  const value = event.currentTarget.value;
                  if (isProviderId(value)) persist(setComponentModel(settings(), id, value));
                }}
              >
                <option value="codex">Codex</option>
                <option value="kimi">Kimi</option>
              </select>
            </label>
          )}
        </For>
      </div>
      <p class={styles['model-settings__note']}>
        Brain Scoring использует формулу, поэтому модель ему не нужна. Подключение провайдеров —
        следующий этап; сейчас сохраняется только выбор.
      </p>
      <Show when={error()}>
        <p role="alert">{error()}</p>
      </Show>
    </section>
  );
}
