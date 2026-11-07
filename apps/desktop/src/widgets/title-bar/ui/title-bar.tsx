import { For, Show } from 'solid-js';
import { isDesktop, windowActions, windowPlatform } from '../../../shared/lib/window-controls';
import type { WindowAction } from '../../../shared/lib/window-controls';
import { createWindowControlModel } from '../model/window-control-model';
import styles from './title-bar.module.scss';

const labels: Record<WindowAction, string> = {
  close: 'Закрыть окно',
  minimize: 'Свернуть окно',
  maximize: 'Развернуть или восстановить окно',
  fullscreen: 'Включить или выключить полноэкранный режим',
};
const symbols: Record<WindowAction, string> = {
  close: '×',
  minimize: '−',
  maximize: '□',
  fullscreen: '⤢',
};

export function TitleBar() {
  const { error, busy, act, reportError } = createWindowControlModel();
  const platform = windowPlatform();
  return (
    <header class={styles['titleBar']} data-platform={platform}>
      <div class={styles['titleBar__drag']} data-tauri-drag-region>
        ✦ PROTEUS <span class={styles['titleBar__label']}>PERSONA LAB / 0.3</span>
      </div>
      <Show
        when={isDesktop()}
        fallback={<span class={styles['titleBar__label']}>WEB PREVIEW</span>}
      >
        <div class={styles['titleBar__controls']}>
          <For each={windowActions(platform)}>
            {(action) => (
              <button
                type="button"
                aria-label={labels[action]}
                data-action={action}
                disabled={busy()}
                onClick={() => void act(action).catch(reportError)}
              >
                <span aria-hidden="true">{symbols[action]}</span>
              </button>
            )}
          </For>
        </div>
      </Show>
      <Show when={error()}>
        <p role="alert">{error()}</p>
      </Show>
    </header>
  );
}
