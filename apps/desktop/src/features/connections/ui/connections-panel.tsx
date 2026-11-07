import { createSignal, For, Show } from 'solid-js';
import { connectionProviders } from '@proteus-int/contracts';
import type { ImportedSource, SourceProvider } from '@proteus-int/contracts';
import { createConnectionSession } from '../model/connection-session';
import { createConnectionMaterials } from '../model/connection-materials';
import { ConfigurationForm } from './configuration-form';
import {
  challenges,
  descriptions,
  names,
  stateLabels,
  capabilityLabels,
} from '../model/connection-labels';
import { formText } from '../../../shared/lib/form-fields';
import styles from './connections-panel.module.scss';

export function ConnectionsPanel(props: {
  onImported?: (provider: SourceProvider, source: ImportedSource) => void;
  onDataCleared?: (provider: SourceProvider) => void;
}) {
  const [editing, setEditing] = createSignal(false);
  const [instagramMode, setInstagramMode] = createSignal<'archive' | 'api'>('archive');
  const {
    desktop,
    selected,
    busy,
    error,
    current,
    qrCode,
    selectProvider,
    activity,
    configure,
    begin,
    advance,
    disconnect,
    openPage,
  } = createConnectionSession(() => setEditing(false));
  const { imported, clearConfirm, cleared, pickImport, clearData, reset } =
    createConnectionMaterials(selected, activity, {
      onImported: (provider, source) => props.onImported?.(provider, source),
      onDataCleared: (provider) => props.onDataCleared?.(provider),
    });
  const challenge = () => challenges[current().status];
  const configurationVisible = () => editing() || current().status === 'unconfigured';

  const runAction = (task: Promise<void>): void => {
    void task.catch(() => activity.fail('Не удалось завершить действие. Попробуйте ещё раз.'));
  };
  const canStartBrowserLogin = () =>
    !configurationVisible() &&
    selected() !== 'instagram' &&
    selected() !== 'telegram' &&
    (current().status === 'disconnected' || current().status === 'error');
  const canStartTelegramLogin = () =>
    selected() === 'telegram' &&
    !configurationVisible() &&
    (current().status === 'disconnected' || current().status === 'error');

  return (
    <section class={styles['connectionsPanel']} aria-label="Подключения источников">
      <div class={styles['connectionsPanel__providers']} role="tablist" aria-label="Источник">
        <For each={connectionProviders}>
          {(provider) => (
            <button
              id={`connection-tab-${provider}`}
              role="tab"
              aria-selected={selected() === provider}
              aria-controls="connection-details"
              disabled={busy()}
              onClick={() => {
                selectProvider(provider);
                setEditing(false);
                reset();
              }}
            >
              {names[provider]}
            </button>
          )}
        </For>
      </div>
      <div id="connection-details" role="tabpanel" aria-labelledby={`connection-tab-${selected()}`}>
        <p class={styles['connectionsPanel__description']}>
          {selected() === 'telegram' && current().status === 'qr-required'
            ? 'Доступная история выбранного чата.'
            : descriptions[selected()]}
        </p>
        <Show
          when={desktop}
          fallback={<p role="status">Подключения доступны в настольном приложении PROTEUS.</p>}
        >
          <p class={styles['connectionsPanel__state']} role="status" aria-live="polite">
            {busy() ? 'Выполняем запрос…' : stateLabels[current().status]}
            <Show when={current().status === 'connected' && current().label}>
              {' '}
              · {current().label}
            </Show>
          </p>
          <Show when={current().message}>
            <p class={styles['connectionsPanel__note']}>{current().message}</p>
          </Show>
          <Show when={selected() === 'instagram'}>
            <div
              class={styles['connectionsPanel__modes']}
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
                      runAction(configure(request));
                    }}
                  />
                )}
              </Show>
            </Show>
            <Show when={canStartBrowserLogin()}>
              <button
                disabled={busy()}
                onClick={() => {
                  runAction(begin());
                }}
              >
                {selected() === 'vk'
                  ? 'Войти в VK через браузер'
                  : `Подключить ${names[selected()]}`}
              </button>
            </Show>
            <Show when={canStartTelegramLogin()}>
              <div class={styles['connectionsPanel__actions']}>
                <button disabled={busy()} onClick={() => runAction(begin('qr'))}>
                  Войти по QR-коду
                </button>
                <button disabled={busy()} onClick={() => runAction(begin('phone'))}>
                  Войти по номеру телефона
                </button>
              </div>
            </Show>
            <Show when={selected() === 'telegram' && current().status === 'qr-required'}>
              <div class={styles['connectionsPanel__qr']}>
                <Show
                  when={qrCode()}
                  fallback={
                    <div class={styles['connectionsPanel__qrPlaceholder']}>
                      Ожидаем актуальный QR-код…
                    </div>
                  }
                >
                  <img src={qrCode()} alt="QR-код входа в Telegram" width="176" height="176" />
                </Show>
                <div>
                  <p>
                    В Telegram на телефоне откройте Настройки → Устройства → Подключить устройство.
                  </p>
                  <p>QR-код обновляется автоматически.</p>
                  <button disabled={busy()} onClick={() => runAction(begin('phone'))}>
                    Отменить вход по QR и войти по номеру
                  </button>
                  <p>Текущая попытка входа будет отменена.</p>
                </div>
              </div>
            </Show>
            <Show when={challenge()} keyed>
              {(step) => (
                <form
                  class={styles['connectionsPanel__form']}
                  autocomplete="off"
                  onSubmit={(event) => {
                    event.preventDefault();
                    const input = formText(new FormData(event.currentTarget), 'challenge');
                    event.currentTarget.reset();
                    runAction(advance(input));
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
            <Show when={current().status === 'pending' && selected() === 'telegram'}>
              <p class={styles['connectionsPanel__note']}>
                Готовим вход в Telegram. Дождитесь обновления состояния.
              </p>
            </Show>
            <Show when={current().status === 'pending' && selected() !== 'telegram'}>
              <Show when={current().userCode}>
                <p class={styles['connectionsPanel__deviceCode']}>
                  Код для страницы подтверждения: <strong>{current().userCode}</strong>
                </p>
              </Show>
              <p class={styles['connectionsPanel__note']}>
                Завершите вход в открывшемся браузере. Здесь появится подтверждённое состояние
                аккаунта.
              </p>
              <Show when={selected() === 'vk'}>
                <button disabled={busy()} onClick={() => runAction(openPage('vk-auth'))}>
                  Открыть браузер ещё раз
                </button>
              </Show>
              <button
                disabled={busy()}
                onClick={() => {
                  runAction(advance());
                }}
              >
                Проверить подключение
              </button>
            </Show>
            <Show when={current().status === 'connected'}>
              <p class={styles['connectionsPanel__note']}>
                Доступ:{' '}
                {current()
                  .capabilities.map((item) => capabilityLabels[item])
                  .filter(Boolean)
                  .join(', ') || 'проверяется для выбранного источника'}
                .
              </p>
            </Show>
            <div class={styles['connectionsPanel__actions']}>
              <Show
                when={
                  !configurationVisible() &&
                  !challenge() &&
                  current().status !== 'pending' &&
                  current().status !== 'qr-required'
                }
              >
                <button disabled={busy()} onClick={() => setEditing(true)}>
                  Настроить {names[selected()]}
                </button>
              </Show>
              <Show
                when={
                  current().status === 'connected' ||
                  current().status === 'pending' ||
                  current().status === 'qr-required' ||
                  challenge()
                }
              >
                <button
                  disabled={busy()}
                  onClick={() => {
                    runAction(disconnect());
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
            <button disabled={busy()} onClick={() => runAction(openPage('instagram-export'))}>
              Открыть экспорт Instagram
            </button>
            <p class={styles['connectionsPanel__note']}>
              Центр аккаунтов → Ваша информация и разрешения → Экспорт информации → На устройство.
              Выберите сообщения, нужные даты и формат JSON. Распакуйте архив и импортируйте JSON
              выбранной переписки или подготовленный текст. Чужая страница целиком в экспорт не
              входит.
            </p>
            <button
              disabled={busy()}
              onClick={() => {
                runAction(pickImport());
              }}
            >
              Импортировать JSON или текст
            </button>
            <Show when={imported()} keyed>
              {(source) => (
                <p class={styles['connectionsPanel__imported']}>
                  {source.label} · {source.documentCount} объекта
                </p>
              )}
            </Show>
          </Show>
          <div class={styles['connectionsPanel__data']}>
            <Show when={clearConfirm()}>
              <p class={styles['connectionsPanel__note']}>
                Подключение сохраняется.
                {selected() === 'telegram' ? ' Кэш Telegram не входит в материалы STALKER.' : ''}
              </p>
            </Show>
            <button type="button" disabled={busy()} onClick={() => runAction(clearData())}>
              {clearConfirm()
                ? `Подтвердить удаление материалов ${names[selected()]}`
                : 'Удалить материалы STALKER'}
            </button>
            <Show when={cleared()}>
              <p>Материалы STALKER удалены.</p>
            </Show>
          </div>
          <Show when={error()}>
            <p class={styles['connectionsPanel__error']} role="alert">
              {error()}
            </p>
          </Show>
          <p class={styles['connectionsPanel__privacy']}>
            Ключи хранятся нативно. Отключение удаляет локальный доступ; отзыв прав у сервиса
            выполняется отдельно.
          </p>
        </Show>
      </div>
    </section>
  );
}
