import { createSignal, onCleanup, onMount, Show } from 'solid-js';
import type { ModelSettings } from '@proteus-int/intelligence';
import type { ImportedSource, SourceProvider } from '@proteus-int/contracts';
import { ModelSettingsPanel } from '../../../features/model-settings';
import { ConnectionsPanel } from '../../../features/connections';
import styles from './settings-dialog.module.scss';

export function SettingsDialog(props: {
  settings: ModelSettings;
  onChange: (settings: ModelSettings) => void;
  onClose: () => void;
  onImported: (provider: SourceProvider, source: ImportedSource) => void;
  onDataCleared: (provider: SourceProvider) => void;
}) {
  const [section, setSection] = createSignal<'models' | 'connections'>('models');
  let dialog: HTMLElement | undefined;
  const previousFocus = document.activeElement;
  onMount(() => dialog?.querySelector<HTMLElement>('[role="tab"]')?.focus());
  onCleanup(() => {
    if (previousFocus instanceof HTMLElement) previousFocus.focus();
  });

  return (
    <section
      ref={(element) => {
        dialog = element;
      }}
      class={styles['settingsDialog']}
      role="dialog"
      aria-modal="true"
      aria-labelledby="settings-title"
      onKeyDown={(event) => {
        if (event.key !== 'Tab' || !dialog) return;
        const targets = [
          ...dialog.querySelectorAll<HTMLElement>('button, input, select, a[href]'),
        ].filter((element) => !element.hasAttribute('disabled') && element.offsetParent !== null);
        const first = targets[0];
        const last = targets.at(-1);
        if (event.shiftKey && document.activeElement === first) {
          event.preventDefault();
          last?.focus();
        } else if (!event.shiftKey && document.activeElement === last) {
          event.preventDefault();
          first?.focus();
        }
      }}
    >
      <header class={styles['settingsDialog__header']}>
        <h2 id="settings-title">Настройки PROTEUS</h2>
        <button aria-label="Закрыть настройки" onClick={() => props.onClose()}>
          ×
        </button>
      </header>
      <div class={styles['settingsDialog__tabs']} role="tablist" aria-label="Раздел настроек">
        <button
          id="models-tab"
          role="tab"
          aria-selected={section() === 'models'}
          aria-controls="settings-panel"
          onClick={() => setSection('models')}
        >
          Модели
        </button>
        <button
          id="connections-tab"
          role="tab"
          aria-selected={section() === 'connections'}
          aria-controls="settings-panel"
          onClick={() => setSection('connections')}
        >
          Подключения
        </button>
      </div>
      <div id="settings-panel" role="tabpanel" aria-labelledby={`${section()}-tab`}>
        <Show
          when={section() === 'models'}
          fallback={
            <ConnectionsPanel onImported={props.onImported} onDataCleared={props.onDataCleared} />
          }
        >
          <ModelSettingsPanel settings={props.settings} onChange={props.onChange} />
        </Show>
      </div>
      <button class={styles['settingsDialog__done']} onClick={() => props.onClose()}>
        Готово ↵
      </button>
    </section>
  );
}
