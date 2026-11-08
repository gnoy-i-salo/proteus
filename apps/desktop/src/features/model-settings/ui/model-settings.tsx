import { createSignal, For, Show } from 'solid-js';
import { componentIds, isProviderId } from '@proteus-int/intelligence';
import type { ModelSettings } from '@proteus-int/intelligence';
import { createModelSettingsEditor } from '../model/model-settings-editor';
import type { CliStatus } from '../../../shared/lib/cli';
import styles from './model-settings.module.scss';
import { LocalModelSettings } from './local-model-settings';
const labels = {
  stalker: 'STALKER',
  'person-fabric': 'Person Fabric',
  person: 'Person',
  'brain-analyzator': 'Brain Analyzator',
};
interface ModelSettingsProps {
  settings: ModelSettings;
  onChange: (settings: ModelSettings) => void;
  hasSources: boolean;
  available: readonly CliStatus[];
}
export function ModelSettingsPanel(props: ModelSettingsProps) {
  const [local, setLocal] = createSignal(false);
  const editor = createModelSettingsEditor({
    settings: () => props.settings,
    hasSources: () => props.hasSources,
    onChange: (settings) => props.onChange(settings),
  });
  let localButton: HTMLButtonElement | undefined;
  return (
    <Show
      when={local()}
      fallback={
        <CliModelSettingsPanel
          {...props}
          editor={editor}
          onLocal={() => setLocal(true)}
          buttonRef={(element) => {
            localButton = element;
          }}
        />
      }
    >
      <LocalModelSettings
        onBack={() => {
          setLocal(false);
          queueMicrotask(() => localButton?.focus());
        }}
      />
    </Show>
  );
}
function CliModelSettingsPanel(
  props: ModelSettingsProps & {
    onLocal: () => void;
    buttonRef: (element: HTMLButtonElement) => void;
    editor: ReturnType<typeof createModelSettingsEditor>;
  },
) {
  const installation = () =>
    props.available.length
      ? props.available
          .map(
            (cli) =>
              `${cli.provider === 'codex' ? 'Codex' : 'Kimi'}: ${cli.available ? 'найден' : 'не найден'}`,
          )
          .join(' · ')
      : 'Статус CLI не проверен';
  return (
    <section class={styles['modelSettings']} aria-label="Настройки моделей">
      <Show
        when={props.editor.pending()}
        fallback={<p>{installation()}. Вход в CLI проверяется при запуске.</p>}
      >
        <div class={styles['modelSettings__warning']} role="alert">
          <p>
            Смена STALKER сбросит разрешения выбранных источников A/B. Задача и контекст останутся;
            источники потребуется выбрать и разрешить обработку заново.
          </p>
          <button type="button" onClick={() => props.editor.confirm()}>
            Применить и сбросить разрешения
          </button>
          <button type="button" onClick={() => props.editor.discard()}>
            Отменить изменение
          </button>
        </div>
      </Show>
      <Show when={!props.editor.pending()}>
        <div class={styles['modelSettings__global']}>
          <label>
            Общий провайдер
            <select
              aria-label="Общий провайдер"
              value={props.editor.globalProvider()}
              onChange={(event) => {
                const id = event.currentTarget.value;
                if (isProviderId(id)) props.editor.selectGlobalProvider(id);
              }}
            >
              <option value="codex">Codex CLI</option>
              <option value="kimi">Kimi CLI</option>
            </select>
          </label>
          <button type="button" onClick={() => props.editor.applyGlobal()}>
            Применить ко всем
          </button>
        </div>
        <p>
          Применение затронет STALKER, Person Fabric, Person и Brain Analyzator; индивидуальные
          alias будут сброшены.
        </p>
      </Show>
      <div class={styles['modelSettings__components']}>
        <div class={styles['modelSettings__row']} aria-hidden="true">
          <span>Компонент</span>
          <span>Провайдер CLI</span>
          <span>Модель / alias</span>
        </div>
        <For each={componentIds}>
          {(id) => (
            <div class={styles['modelSettings__row']}>
              <span>{labels[id]}</span>
              <select
                aria-label={`Провайдер ${labels[id]}`}
                value={props.editor.settings().components[id]}
                onChange={(event) => {
                  const value = event.currentTarget.value;
                  if (isProviderId(value)) props.editor.changeProvider(id, value);
                }}
              >
                <option value="codex">Codex</option>
                <option value="kimi">Kimi</option>
              </select>
              <input
                aria-label={`Название модели ${labels[id]}`}
                value={props.editor.settings().aliases[id]}
                maxLength={120}
                placeholder="По умолчанию CLI"
                onInput={(event) => props.editor.changeAlias(id, event.currentTarget.value)}
              />
            </div>
          )}
        </For>
      </div>
      <div class={styles.modelSettings__footer}>
        <p class={styles.modelSettings__note}>
          Пустой alias: модель CLI по умолчанию. BSC — без модели.
        </p>
        <button
          ref={(element) => props.buttonRef(element)}
          type="button"
          onClick={() => props.onLocal()}
        >
          Локальный поиск
        </button>
      </div>
    </section>
  );
}
