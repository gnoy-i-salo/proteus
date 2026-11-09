import { HistoryLegacySeries } from './history-legacy-series';
import { For, Show } from 'solid-js';
import type { PreparedStoredResult, StoredResult } from '@proteus-int/pipeline';
import type { ExperimentHistoryModel, OpenedExperiment } from '../model/experiment-history';
import { HistoryOriginal, HistorySeries } from './history-results';
import styles from './experiment-history.module.scss';

function OpenedHistory(props: {
  readonly value: OpenedExperiment<PreparedStoredResult>;
  readonly onReuse: (value: StoredResult) => void;
}) {
  const stored = () => props.value.result.stored;
  const view = () => props.value.result.view;
  const modern = () => {
    const value = view();
    return value.sourceVersion === 4 || value.sourceVersion === 5 ? value.result : undefined;
  };
  const legacy = () => {
    const value = view();
    return value.sourceVersion === 2 || value.sourceVersion === 3 ? value.result : undefined;
  };
  const original = () => {
    const value = view();
    return value.sourceVersion === 1 ? value.result : undefined;
  };
  const originalExport = () => {
    const value = stored();
    return value.sourceVersion !== 1 ? value.result.exportMetadata : undefined;
  };
  const status = () => {
    const value = stored();
    if (value.sourceVersion === 1) return 'не сохранялся в этой версии';
    return { complete: 'завершён', incomplete: 'не завершён', cancelled: 'отменён' }[
      value.result.status
    ];
  };
  return (
    <div class={styles.experimentHistory__opened}>
      <header class={styles.experimentHistory__metadata}>
        <h2>Сохранённый запуск {props.value.id}</h2>
        <p>Версия файла: {stored().sourceVersion}</p>
        <p>Статус: {status()}</p>
        <p>
          Дата результата:{' '}
          <time dateTime={stored().result.finishedAt}>{stored().result.finishedAt}</time>
        </p>
        <p>
          {originalExport()
            ? originalExport()!.redacted
              ? 'Сохранённый экспорт содержит изъятия.'
              : 'В сохранённом экспорте изъятия не отмечены.'
            : 'Статус экспорта в исходном файле не сохранён.'}
        </p>
        <details>
          <summary>Источники и ограничения сохранения</summary>
          <p>Локальный файл прочитан. Просмотр не запускает эксперимент и не изменяет оригинал.</p>
          <Show when={originalExport()}>
            {(metadata) => (
              <p>
                Изъято примеров стиля: {metadata().omittedStyleExemplars}; цитат источников:{' '}
                {metadata().omittedSourceExcerpts}. Оценки относятся к исходным ответам.
              </p>
            )}
          </Show>
          <p>
            Доступ к исходникам сейчас не проверен. Удалённые, отозванные и отсутствующие материалы
            не загружаются из источников. Просмотр применяет ограничения экспорта; это не точное
            воспроизведение закрытого содержимого.
          </p>
        </details>
        <button type="button" onClick={() => props.onReuse(props.value.result.view)}>
          Новый запуск из этих условий
        </button>
        <p>
          Только заполнение формы. Источники нужно выбрать заново, новый протокол — подтвердить.
        </p>
      </header>
      <Show when={modern()}>{(result) => <HistorySeries result={result()} />}</Show>
      <Show when={legacy()}>{(result) => <HistoryLegacySeries result={result()} />}</Show>
      <Show when={original()}>{(result) => <HistoryOriginal result={result()} />}</Show>
    </div>
  );
}

export function ExperimentHistory(props: {
  readonly history: ExperimentHistoryModel<PreparedStoredResult>;
  readonly onClose: () => void;
  readonly onError: (message: string) => void;
  readonly onReuse: (value: StoredResult) => void;
}) {
  return (
    <section class={styles.experimentHistory} aria-label="История запусков">
      <header class={styles.experimentHistory__header}>
        <h1>История запусков</h1>
        <button type="button" onClick={() => props.onClose()}>
          Вернуться к эксперименту
        </button>
      </header>
      <div class={styles.experimentHistory__layout}>
        <aside class={styles.experimentHistory__list}>
          <button
            type="button"
            disabled={props.history.loading()}
            onClick={() =>
              void props.history
                .refresh()
                .catch(() =>
                  props.onError('Не удалось выполнить действие истории. Повторите попытку.'),
                )
            }
          >
            Обновить историю
          </button>
          <Show when={props.history.loading()}>
            <p role="status">Читаем список…</p>
          </Show>
          <Show when={props.history.listError()}>
            <p role="alert">{props.history.listError()}</p>
          </Show>
          <Show when={props.history.page()}>
            {(page) => (
              <>
                <Show when={!page().entries.length}>
                  <p>Сохранённых запусков пока нет.</p>
                </Show>
                <Show when={page().scanTruncated}>
                  <p role="status">
                    Показана только часть каталога. Список не является полным снимком истории.
                  </p>
                </Show>
                <For each={page().entries}>
                  {(entry) => (
                    <div class={styles.experimentHistory__entry}>
                      <span>{entry.id}</span>
                      <button
                        type="button"
                        disabled={!entry.ok || props.history.openingId() === entry.id}
                        aria-label={`Открыть ${entry.id}`}
                        onClick={() =>
                          void props.history
                            .open(entry.id)
                            .catch(() =>
                              props.onError(
                                'Не удалось выполнить действие истории. Повторите попытку.',
                              ),
                            )
                        }
                      >
                        {entry.ok ? 'Открыть' : 'Файл недоступен'}
                      </button>
                    </div>
                  )}
                </For>
                <nav class={styles.experimentHistory__tabs} aria-label="Страницы истории">
                  <button
                    type="button"
                    disabled={props.history.loading() || props.history.offset() === 0}
                    onClick={() =>
                      void props.history
                        .previousPage()
                        .catch(() =>
                          props.onError(
                            'Не удалось выполнить действие истории. Повторите попытку.',
                          ),
                        )
                    }
                  >
                    Назад
                  </button>
                  <button
                    type="button"
                    disabled={props.history.loading() || page().nextOffset === null}
                    onClick={() =>
                      void props.history
                        .nextPage()
                        .catch(() =>
                          props.onError(
                            'Не удалось выполнить действие истории. Повторите попытку.',
                          ),
                        )
                    }
                  >
                    Далее
                  </button>
                </nav>
              </>
            )}
          </Show>
          <Show when={props.history.openingId()}>
            <p role="status">Открываем {props.history.openingId()}…</p>
            <button type="button" onClick={() => props.history.cancel()}>
              Отменить открытие
            </button>
          </Show>
          <Show when={props.history.openError()}>
            <p role="alert">{props.history.openError()}</p>
          </Show>
        </aside>
        <Show
          when={props.history.opened()}
          keyed
          fallback={
            <p class={styles.experimentHistory__empty}>
              Выберите сохранённый запуск. Его просмотр не вызывает модели.
            </p>
          }
        >
          {(value) => <OpenedHistory value={value} onReuse={props.onReuse} />}
        </Show>
      </div>
    </section>
  );
}
