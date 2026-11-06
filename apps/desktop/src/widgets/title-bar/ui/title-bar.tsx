import { createSignal, Show } from 'solid-js';
import { controlWindow, isDesktop } from '../../../shared/lib/window-controls';
import styles from './title-bar.module.scss';
export function TitleBar() {
  const [error, setError] = createSignal('');
  const act = (action: 'minimize' | 'maximize' | 'close') => {
    void controlWindow(action).catch(() =>
      setError('Не удалось изменить окно. Попробуйте ещё раз.'),
    );
  };
  return (
    <>
      <header class={styles['title-bar']}>
        <div class={styles['title-bar__drag']} data-tauri-drag-region>
          ✦ PROTEUS <span class={styles['title-bar__label']}>PERSONA LAB / 0.3</span>
        </div>
        <Show
          when={isDesktop()}
          fallback={<span class={styles['title-bar__label']}>WEB PREVIEW</span>}
        >
          <div class={styles['title-bar__controls']}>
            <button aria-label="Свернуть окно" onClick={() => act('minimize')}>
              _
            </button>
            <button aria-label="Развернуть окно" onClick={() => act('maximize')}>
              □
            </button>
            <button aria-label="Закрыть окно" onClick={() => act('close')}>
              ×
            </button>
          </div>
        </Show>
      </header>
      <Show when={error()}>
        <p role="alert">{error()}</p>
      </Show>
    </>
  );
}
