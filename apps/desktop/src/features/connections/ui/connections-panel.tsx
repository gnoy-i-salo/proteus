import { createSignal, For, onCleanup, onMount, Show } from 'solid-js';
import { connectionProviders } from '@proteus-int/contracts';
import type { ConnectionStatus, ImportedSource, SourceProvider } from '@proteus-int/contracts';
import {
  advanceConnection,
  beginConnection,
  configureConnection,
  connectionStatuses,
  disconnectConnection,
  importSource,
  clearSourceData,
} from '../../../shared/lib/connections';
import { isDesktop } from '../../../shared/lib/window-controls';
import { ConfigurationForm } from './configuration-form';
import {
  challenges,
  descriptions,
  names,
  stateLabels,
  capabilityLabels,
} from '../model/connection-labels';
import styles from './connections-panel.module.scss';

export function ConnectionsPanel(props: {
  onImported?: (provider: SourceProvider, source: ImportedSource) => void;
  onDataCleared?: (provider: SourceProvider) => void;
}) {
  const desktop = isDesktop();
  const [selected, setSelected] = createSignal<SourceProvider>('telegram');
  const [statuses, setStatuses] = createSignal<readonly ConnectionStatus[]>([]);
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal('');
  const [editing, setEditing] = createSignal(false);
  const [instagramMode, setInstagramMode] = createSignal<'archive' | 'api'>('archive');
  const [imported, setImported] = createSignal<ImportedSource>();
  const [clearConfirm, setClearConfirm] = createSignal(false);
  const [cleared, setCleared] = createSignal(false);
  let disposed = false;
  let revision = 0;
  let nextPoll = 0;
  const current = (): ConnectionStatus =>
    statuses().find((item) => item.provider === selected()) ?? {
      provider: selected(),
      status: 'unconfigured',
      capabilities: [],
    };
  const challenge = () => challenges[current().status];
  const configurationVisible = () => editing() || current().status === 'unconfigured';

  const refresh = async () => {
    if (!desktop || busy()) return;
    const version = revision;
    try {
      const result = await connectionStatuses();
      if (!disposed && version === revision) {
        setStatuses(result);
        const pending = result.find(
          (item) => item.provider === 'github' && item.status === 'pending',
        );
        if (pending && nextPoll === 0)
          nextPoll = Date.now() + Math.max(1000, pending['poll-after-ms'] ?? 5000);
      }
    } catch {
      if (!disposed && version === revision)
        setError('Не удалось получить состояние подключений. Попробуйте ещё раз.');
    }
  };
  const perform = async (operation: () => Promise<ConnectionStatus>) => {
    if (busy()) return;
    const version = ++revision;
    setBusy(true);
    setError('');
    try {
      const result = await operation();
      if (!disposed && version === revision) {
        setStatuses((items) => [
          ...items.filter((item) => item.provider !== result.provider),
          result,
        ]);
        setEditing(false);
        nextPoll = Date.now() + Math.max(1000, result['poll-after-ms'] ?? 5000);
      }
    } catch {
      if (!disposed && version === revision)
        setError('Не удалось выполнить подключение. Проверьте данные и повторите попытку.');
    } finally {
      if (!disposed && version === revision) setBusy(false);
    }
  };
  const pickImport = async () => {
    if (busy()) return;
    ++revision;
    setBusy(true);
    setError('');
    setCleared(false);
    const provider = selected();
    try {
      const source = await importSource(provider);
      if (!disposed && source) {
        setImported(source);
        props.onImported?.(provider, source);
      }
    } catch {
      if (!disposed)
        setError('Не удалось импортировать файл. Выберите поддерживаемый JSON экспорт или текст.');
    } finally {
      if (!disposed) setBusy(false);
    }
  };
  const clearData = async () => {
    if (busy()) return;
    if (!clearConfirm()) {
      setClearConfirm(true);
      return;
    }
    ++revision;
    setBusy(true);
    setError('');
    const provider = selected();
    try {
      await clearSourceData(provider);
      if (!disposed) {
        setImported(undefined);
        setCleared(true);
        setClearConfirm(false);
        props.onDataCleared?.(provider);
      }
    } catch {
      if (!disposed) setError('Не удалось удалить загруженные данные. Повторите попытку.');
    } finally {
      if (!disposed) setBusy(false);
    }
  };
  onMount(() => {
    void refresh();
    const timer = window.setInterval(() => {
      if (!desktop || busy()) return;
      if (selected() === 'github' && current().status === 'pending' && Date.now() >= nextPoll)
        void perform(() => advanceConnection('github'));
      else void refresh();
    }, 1500);
    onCleanup(() => {
      disposed = true;
      revision++;
      window.clearInterval(timer);
    });
  });

  return (
    <section class={styles['connections-panel']} aria-label="Подключения источников">
      <div class={styles['connections-panel__providers']} role="tablist" aria-label="Источник">
        <For each={connectionProviders}>
          {(provider) => (
            <button
              id={`connection-tab-${provider}`}
              role="tab"
              aria-selected={selected() === provider}
              aria-controls="connection-details"
              disabled={busy()}
              onClick={() => {
                setSelected(provider);
                setEditing(false);
                setError('');
                setImported(undefined);
                setClearConfirm(false);
                setCleared(false);
              }}
            >
              {names[provider]}
            </button>
          )}
        </For>
      </div>
      <div id="connection-details" role="tabpanel" aria-labelledby={`connection-tab-${selected()}`}>
        <p class={styles['connections-panel__description']}>{descriptions[selected()]}</p>
        <Show
          when={desktop}
          fallback={<p role="status">Подключения доступны в настольном приложении PROTEUS.</p>}
        >
          <p class={styles['connections-panel__state']} role="status" aria-live="polite">
            {busy() ? 'Выполняем запрос…' : stateLabels[current().status]}
            <Show when={current().status === 'connected' && current().label}>
              {' '}
              · {current().label}
            </Show>
          </p>
          <Show when={current().message}>
            <p class={styles['connections-panel__note']}>{current().message}</p>
          </Show>
          <Show when={selected() === 'instagram'}>
            <div
              class={styles['connections-panel__modes']}
              role="tablist"
              aria-label="Способ доступа к Instagram"
            >
              <button
                role="tab"
                aria-selected={instagramMode() === 'archive'}
                onClick={() => setInstagramMode('archive')}
              >
                Архив
              </button>
              <button
                role="tab"
                aria-selected={instagramMode() === 'api'}
                onClick={() => setInstagramMode('api')}
              >
                API Business / Creator
              </button>
            </div>
          </Show>
          <Show when={selected() !== 'instagram' || instagramMode() === 'api'}>
            <Show
              when={
                configurationVisible() ||
                (selected() === 'instagram' && current().status !== 'connected')
              }
            >
              <Show when={selected()} keyed>
                {(provider) => (
                  <ConfigurationForm
                    provider={provider}
                    status={current()}
                    busy={busy()}
                    onSubmit={(request) => {
                      void perform(() => configureConnection(request));
                    }}
                  />
                )}
              </Show>
            </Show>
            <Show
              when={
                !configurationVisible() &&
                selected() !== 'instagram' &&
                (current().status === 'disconnected' || current().status === 'error')
              }
            >
              <button
                disabled={busy()}
                onClick={() => {
                  void perform(() => beginConnection(selected()));
                }}
              >
                Подключить {names[selected()]}
              </button>
            </Show>
            <Show when={challenge()} keyed>
              {(step) => (
                <form
                  class={styles['connections-panel__form']}
                  autocomplete="off"
                  onSubmit={(event) => {
                    event.preventDefault();
                    const input = String(new FormData(event.currentTarget).get('challenge') ?? '');
                    event.currentTarget.reset();
                    void perform(() => advanceConnection(selected(), input));
                  }}
                >
                  <label>
                    {step.label}
                    <input
                      name="challenge"
                      aria-label={step.label}
                      type={step.type}
                      autocomplete="off"
                      required
                      maxLength={256}
                      disabled={busy()}
                    />
                  </label>
                  <button type="submit" disabled={busy()}>
                    Продолжить
                  </button>
                </form>
              )}
            </Show>
            <Show when={current().status === 'pending'}>
              <Show when={current()['user-code']}>
                <p class={styles['connections-panel__device-code']}>
                  Код для страницы подтверждения: <strong>{current()['user-code']}</strong>
                </p>
              </Show>
              <p class={styles['connections-panel__note']}>
                Завершите вход в открывшемся браузере. Здесь появится подтверждённое состояние
                аккаунта.
              </p>
              <button
                disabled={busy()}
                onClick={() => {
                  void perform(() => advanceConnection(selected()));
                }}
              >
                Проверить подключение
              </button>
            </Show>
            <Show when={current().status === 'connected'}>
              <p class={styles['connections-panel__note']}>
                Доступ:{' '}
                {current()
                  .capabilities.map((item) => capabilityLabels[item])
                  .filter(Boolean)
                  .join(', ') || 'проверяется для выбранного источника'}
                .
              </p>
            </Show>
            <div class={styles['connections-panel__actions']}>
              <Show
                when={!configurationVisible() && !challenge() && current().status !== 'pending'}
              >
                <button disabled={busy()} onClick={() => setEditing(true)}>
                  Настроить {names[selected()]}
                </button>
              </Show>
              <Show
                when={
                  current().status === 'connected' || current().status === 'pending' || challenge()
                }
              >
                <button
                  disabled={busy()}
                  onClick={() => {
                    void perform(() => disconnectConnection(selected()));
                  }}
                >
                  {current().status === 'connected'
                    ? `Отключить ${names[selected()]}`
                    : 'Отменить подключение'}
                </button>
              </Show>
            </div>
          </Show>
          <Show when={selected() === 'instagram' && instagramMode() === 'archive'}>
            <p class={styles['connections-panel__note']}>
              Экспортируйте данные Instagram на устройство и распакуйте архив. Выберите JSON с
              сообщениями или подготовленный текст. Чужая страница целиком в экспорт не входит.
            </p>
            <button
              disabled={busy()}
              onClick={() => {
                void pickImport();
              }}
            >
              Импортировать JSON или текст
            </button>
            <Show when={imported()} keyed>
              {(source) => (
                <p class={styles['connections-panel__imported']}>
                  {source.label} · {source['document-count']} объекта
                </p>
              )}
            </Show>
          </Show>
          <div class={styles['connections-panel__data']}>
            <Show when={clearConfirm()}>
              <p class={styles['connections-panel__note']}>
                Подключение сохраняется.
                {selected() === 'telegram' ? ' Кэш Telegram не входит в материалы STALKER.' : ''}
              </p>
            </Show>
            <button type="button" disabled={busy()} onClick={() => void clearData()}>
              {clearConfirm()
                ? `Подтвердить удаление материалов ${names[selected()]}`
                : 'Удалить материалы STALKER'}
            </button>
            <Show when={cleared()}>
              <p>Материалы STALKER удалены.</p>
            </Show>
          </div>
          <Show when={error()}>
            <p class={styles['connections-panel__error']} role="alert">
              {error()}
            </p>
          </Show>
          <p class={styles['connections-panel__privacy']}>
            Ключи хранятся нативно. Отключение удаляет локальный доступ; отзыв прав у сервиса
            выполняется отдельно.
          </p>
        </Show>
      </div>
    </section>
  );
}
