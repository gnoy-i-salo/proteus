import { For } from 'solid-js';
import {
  componentIds,
  isProviderId,
  setComponentModel,
  setGlobalModel,
  setModelAlias,
} from '@proteus-int/intelligence';
import type { ModelSettings } from '@proteus-int/intelligence';
import styles from './model-settings.module.scss';
const labels = {
  stalker: 'STALKER',
  'person-fabric': 'Person Fabric',
  person: 'Person',
  'brain-analyzator': 'Brain Analyzator',
};
export function ModelSettingsPanel(props: {
  settings: ModelSettings;
  onChange: (settings: ModelSettings) => void;
}) {
  return (
    <section class={styles['model-settings']} aria-label="Настройки моделей">
      <p>Установленные CLI · авторизация остаётся в Codex / Kimi</p>
      <div class={styles['model-settings__global']}>
        <label>
          Общая модель
          <select
            aria-label="Общая модель"
            value={props.settings['global-model']}
            onChange={(event) => {
              const id = event.currentTarget.value;
              if (isProviderId(id)) props.onChange(setGlobalModel(props.settings, id));
            }}
          >
            <option value="codex">Codex CLI</option>
            <option value="kimi">Kimi CLI</option>
          </select>
        </label>
        <button
          onClick={() =>
            props.onChange(setGlobalModel(props.settings, props.settings['global-model']))
          }
        >
          Применить ко всем
        </button>
      </div>
      <div class={styles['model-settings__components']}>
        <For each={componentIds}>
          {(id) => (
            <div class={styles['model-settings__row']}>
              <label>
                {labels[id]}
                <select
                  aria-label={`Модель ${labels[id]}`}
                  value={props.settings.components[id]}
                  onChange={(event) => {
                    const value = event.currentTarget.value;
                    if (isProviderId(value))
                      props.onChange(setComponentModel(props.settings, id, value));
                  }}
                >
                  <option value="codex">Codex</option>
                  <option value="kimi">Kimi</option>
                </select>
              </label>
              <label>
                Модель / alias
                <input
                  aria-label={`Название модели ${labels[id]}`}
                  value={props.settings.aliases[id]}
                  maxLength={120}
                  placeholder="По умолчанию CLI"
                  onInput={(event) =>
                    props.onChange(setModelAlias(props.settings, id, event.currentTarget.value))
                  }
                />
              </label>
            </div>
          )}
        </For>
      </div>
      <p class={styles['model-settings__note']}>
        Пустое название использует модель CLI по умолчанию. Brain Scoring считает BSC по формуле и
        не вызывает модель.
      </p>
    </section>
  );
}
