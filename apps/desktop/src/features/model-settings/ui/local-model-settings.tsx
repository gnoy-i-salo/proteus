import { onMount, Show } from 'solid-js';
import { createLocalModelManager } from '../model/local-model-manager';
import { isDesktop } from '../../../shared/lib/window-controls';
import styles from './local-model-settings.module.scss';

export function LocalModelSettings(props: { onBack: () => void }) {
  const manager = createLocalModelManager();
  let back: HTMLButtonElement | undefined;
  const perform = (action: () => Promise<void>) => {
    void action().catch(manager.fail);
  };
  onMount(() => {
    back?.focus();
    perform(manager.refresh);
  });
  const disabled = () => !isDesktop() || Boolean(manager.busy());
  const disk = () =>
    ({ unknown: 'не проверены', missing: 'ещё не загружены', ready: 'проверены' })[
      manager.packageState()
    ];
  const actions = {
    refreshing: 'Читаем состояние…',
    checking: 'Проверяем файлы…',
    downloading: 'Загружаем и проверяем…',
    unloading: 'Освобождаем память…',
  };
  return (
    <section class={styles.localModel} aria-label="Локальная модель поиска">
      <button
        ref={(element) => {
          back = element;
        }}
        type="button"
        onClick={() => props.onBack()}
      >
        Назад к моделям CLI
      </button>
      <h3>Локальный поиск</h3>
      <p>multilingual-e5-small ищет близкие по смыслу фрагменты на этом компьютере.</p>
      <Show when={manager.state()}>
        {(state) => (
          <p>
            Загрузка: {Math.ceil(state().packageBytes / 1024 ** 2)} МиБ с Hugging Face, только по
            вашему запросу.
          </p>
        )}
      </Show>
      <dl>
        <div>
          <dt>Файлы:</dt>
          <dd>{disk()}</dd>
        </div>
        <div>
          <dt>В памяти:</dt>
          <dd>
            {!manager.state()
              ? 'состояние неизвестно'
              : manager.state()?.runtimeLoaded
                ? 'загружена'
                : 'не загружена'}
          </dd>
        </div>
      </dl>
      <div class={styles.localModel__actions}>
        <button type="button" disabled={disabled()} onClick={() => perform(manager.inspect)}>
          Проверить файлы
        </button>
        <button
          type="button"
          disabled={disabled() || manager.state()?.operationActive}
          onClick={() => perform(manager.download)}
        >
          Загрузить модель
        </button>
        <button
          type="button"
          disabled={
            disabled() || !manager.state()?.runtimeLoaded || manager.state()?.operationActive
          }
          onClick={() => perform(manager.unload)}
        >
          Освободить память
        </button>
        <button type="button" disabled={disabled()} onClick={() => perform(manager.refresh)}>
          Обновить состояние
        </button>
      </div>
      <Show when={manager.busy()}>
        {(busy) => (
          <div class={styles.localModel__activity}>
            <p role="status">
              {manager.stopping() ? 'Ожидаем остановки…' : actions[busy() as keyof typeof actions]}
              <Show when={!manager.stopping() && manager.progress()}>
                {(progress) =>
                  ` Проверено файлов: ${progress().artifactsVerified} из 3. Счётчик обновляется после проверки файла.`
                }
              </Show>
            </p>
            <button type="button" disabled={manager.stopping()} onClick={manager.cancel}>
              Отменить операцию
            </button>
          </div>
        )}
      </Show>
      <Show when={!manager.busy() && manager.message()}>
        <p role="status">{manager.message()}</p>
      </Show>
      <Show when={!manager.busy()}>
        <p class={styles.localModel__note}>
          Без модели работает поиск по словам. Разрешения источников не меняются.
        </p>
      </Show>
    </section>
  );
}
