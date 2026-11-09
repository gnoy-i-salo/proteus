import { For, Show } from 'solid-js';
import type { createSourceImport } from '../model/source-import';
import styles from './source-preparation.module.scss';
const statuses = {
  queued: 'Импорт ожидает запуска',
  running: 'Импорт выполняется',
  paused: 'Импорт приостановлен',
  cancelled: 'Импорт отменён',
  failed: 'Ошибка импорта',
  completed: 'Импорт завершён',
} as const;
export function SourceImportProgress(props: {
  readonly model: ReturnType<typeof createSourceImport>;
}) {
  const coverage = () => props.model.snapshot()?.coverage ?? props.model.job()?.coverage;
  return (
    <Show when={props.model.job()}>
      {(job) => (
        <section aria-label="Состояние импорта">
          <p role="status">{statuses[job().status]}</p>
          <Show when={coverage()}>
            {(value) => (
              <div class={styles.sourcePreparation__coverage}>
                <p>
                  Загружено: {value().loadedDocuments}
                  {value().totalDocuments === null
                    ? ' · общий объём неизвестен'
                    : ` / ${value().totalDocuments}`}
                </p>
                <p>Проиндексировано: {value().indexedDocuments}</p>
                <p>Изучено моделью: {value().analyzedDocuments}</p>
                <p>Использовано в ответах: {value().usedDocuments}</p>
                <For each={value().limitations}>{(text) => <p>{text}</p>}</For>
              </div>
            )}
          </Show>
          <Show when={job().error}>{(error) => <p role="alert">{error().message}</p>}</Show>
          <div class={styles.sourcePreparation__actions}>
            <Show when={['queued', 'running'].includes(job().status)}>
              <button
                disabled={props.model.busy()}
                onClick={() => void props.model.pause().catch(props.model.reportError)}
              >
                Пауза
              </button>
            </Show>
            <Show when={job().status === 'paused'}>
              <button
                disabled={props.model.busy()}
                onClick={() => void props.model.resume().catch(props.model.reportError)}
              >
                Продолжить импорт
              </button>
            </Show>
            <Show when={job().status === 'failed' && job().error?.retryable}>
              <button
                disabled={props.model.busy()}
                onClick={() => void props.model.retry().catch(props.model.reportError)}
              >
                Повторить импорт
              </button>
            </Show>
            <Show when={['queued', 'running', 'paused'].includes(job().status)}>
              <button onClick={() => void props.model.cancel().catch(props.model.reportError)}>
                Отменить импорт
              </button>
            </Show>
            <button
              disabled={props.model.busy()}
              onClick={() => void props.model.refresh().catch(props.model.reportError)}
            >
              Обновить состояние
            </button>
            <Show when={job().status === 'completed' && !props.model.snapshot()}>
              <button
                disabled={props.model.busy()}
                onClick={() => void props.model.prepare().catch(props.model.reportError)}
              >
                Подготовить корпус
              </button>
            </Show>
          </div>
          <Show when={props.model.snapshot()}>
            {(snapshot) => (
              <div>
                <p role="status">Корпус подготовлен</p>
                <p>Версия корпуса: {snapshot().id}</p>
                <p>Эпизодов: {snapshot().episodeIds.length}</p>
              </div>
            )}
          </Show>
        </section>
      )}
    </Show>
  );
}
