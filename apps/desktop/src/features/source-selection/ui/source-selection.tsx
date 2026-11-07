import { For, onCleanup, onMount, Show } from 'solid-js';
import { connectionProviders } from '@proteus-int/contracts';
import type { ImportedSource, SourceGrant, SourceProvider } from '@proteus-int/contracts';
import { createSourceSelectionModel } from '../model/source-selection-model';
import { isDesktop } from '../../../shared/lib/window-controls';
import styles from './source-selection.module.scss';

const providers: Record<SourceProvider, { name: string; hint: string }> = {
  telegram: {
    name: 'Telegram',
    hint: 'ID выбранного чата, например −1001234567890. Для чужих реплик укажите ID автора.',
  },
  vk: {
    name: 'VK',
    hint: 'Пост wall−123_456 или страница id123. Доступ зависит от разрешений приложения VK.',
  },
  github: { name: 'GitHub', hint: 'owner/repo или ссылка на репозиторий. Автор — логин GitHub.' },
  instagram: {
    name: 'Instagram',
    hint: 'JSON из вашего архива либо ID своего подключённого Business / Creator аккаунта.',
  },
};

export function SourceSelection(props: {
  letter: string;
  model: string;
  modelProcessingAvailable: boolean;
  grants: readonly SourceGrant[];
  imports: readonly { provider: SourceProvider; source: ImportedSource }[];
  onImported: (provider: SourceProvider, source: ImportedSource) => void;
  onChange: (grants: readonly SourceGrant[]) => void;
  onClose: () => void;
}) {
  const {
    provider,
    resource,
    consent,
    busy,
    error,
    selectedGrant,
    adding,
    current,
    setConsent,
    setSelectedGrant,
    setAdding,
    selectProvider,
    changeResource,
    pick,
    add,
    remove,
  } = createSourceSelectionModel({
    grants: () => props.grants,
    modelProcessingAvailable: () => props.modelProcessingAvailable,
    onImported: (provider, source) => props.onImported(provider, source),
    onChange: (grants) => props.onChange(grants),
  });
  let dialog: HTMLElement | undefined;
  const previousFocus = document.activeElement;
  onMount(() => dialog?.querySelector<HTMLButtonElement>('button')?.focus());
  onCleanup(() => {
    if (previousFocus instanceof HTMLElement) previousFocus.focus();
  });
  return (
    <section
      class={styles['source-selection']}
      role="dialog"
      aria-modal="true"
      aria-labelledby="sources-title"
      ref={(element) => {
        dialog = element;
      }}
      onKeyDown={(event) => {
        if (event.key === 'Escape' && !busy()) props.onClose();
        if (event.key !== 'Tab' || !dialog) return;
        const targets = [...dialog.querySelectorAll<HTMLElement>('button, input, select')].filter(
          (item) => !item.hasAttribute('disabled') && item.offsetParent !== null,
        );
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
      <header>
        <h2 id="sources-title">Источники персоны {props.letter}</h2>
        <button
          type="button"
          disabled={busy()}
          aria-label="Закрыть источники"
          onClick={() => props.onClose()}
        >
          ×
        </button>
      </header>
      <Show
        when={isDesktop()}
        fallback={<p>Выбор подключённых источников доступен в настольном приложении PROTEUS.</p>}
      >
        <Show when={!props.modelProcessingAvailable}>
          <p class={styles['source-selection__error']} role="alert">
            Для подключённых источников выберите Codex для всех компонентов в настройках. Поддержка
            Kimi для этих материалов пока недоступна.
          </p>
        </Show>
        <Show when={props.grants.length > 0}>
          <div
            class={styles['source-selection__tabs']}
            role="tablist"
            aria-label="Режим выбора источника"
          >
            <button
              type="button"
              role="tab"
              aria-selected={!adding()}
              disabled={busy()}
              onClick={() => setAdding(false)}
            >
              Добавлено {props.grants.length}
            </button>
            <button
              type="button"
              role="tab"
              aria-selected={adding()}
              disabled={busy() || props.grants.length >= 5}
              onClick={() => setAdding(true)}
            >
              Новый источник
            </button>
          </div>
        </Show>
        <Show when={adding()}>
          <form
            onSubmit={(event) => {
              event.preventDefault();
              const fields = new FormData(event.currentTarget);
              void add({
                author: String(fields.get('author') ?? ''),
                from: String(fields.get('from') ?? ''),
                to: String(fields.get('to') ?? ''),
              });
            }}
          >
            <div class={styles['source-selection__provider']}>
              <label>
                Платформа
                <select
                  aria-label="Платформа источника"
                  value={provider()}
                  disabled={busy()}
                  onChange={(event) => {
                    const provider = connectionProviders.find(
                      (value) => value === event.currentTarget.value,
                    );
                    if (provider) selectProvider(provider);
                  }}
                >
                  <For each={connectionProviders}>
                    {(value) => <option value={value}>{providers[value].name}</option>}
                  </For>
                </select>
              </label>
              <Show when={props.imports.some((item) => item.provider === provider())}>
                <label>
                  Импортированный файл
                  <select
                    aria-label="Импортированный файл"
                    disabled={busy()}
                    value={resource().startsWith('import:') ? resource() : ''}
                    onChange={(event) => {
                      changeResource(event.currentTarget.value);
                    }}
                  >
                    <option value="">Выберите файл</option>
                    <For each={props.imports.filter((item) => item.provider === provider())}>
                      {(item) => (
                        <option value={item.source.id}>
                          {item.source.label} · {item.source.documentCount}
                        </option>
                      )}
                    </For>
                  </select>
                </label>
              </Show>
            </div>
            <p class={styles['source-selection__hint']}>{providers[provider()].hint}</p>
            <div class={styles['source-selection__resource']}>
              <label>
                Ресурс
                <input
                  aria-label="Ресурс"
                  required
                  maxLength={300}
                  value={resource()}
                  disabled={busy()}
                  onInput={(event) => {
                    changeResource(event.currentTarget.value);
                  }}
                />
              </label>
              <button type="button" disabled={busy()} onClick={() => void pick()}>
                Импорт JSON / текста
              </button>
            </div>
            <div class={styles['source-selection__range']}>
              <label>
                Автор
                <input
                  aria-label="Автор"
                  name="author"
                  maxLength={100}
                  disabled={busy()}
                  placeholder="ID / логин, если известен"
                  onInput={() => setConsent(false)}
                />
              </label>
              <label>
                С (UTC)
                <input
                  aria-label="Дата начала"
                  name="from"
                  type="date"
                  disabled={busy()}
                  onInput={() => setConsent(false)}
                />
              </label>
              <label>
                По (UTC)
                <input
                  aria-label="Дата конца"
                  name="to"
                  type="date"
                  disabled={busy()}
                  onInput={() => setConsent(false)}
                />
              </label>
            </div>
            <label class={styles['source-selection__consent']}>
              <input
                type="checkbox"
                checked={consent()}
                disabled={busy() || !props.modelProcessingAvailable}
                onChange={(event) => setConsent(event.currentTarget.checked)}
              />
              <span>
                Разрешаю передать выбранные материалы в {props.model} для создания персоны.
                Использую материалы, которые вправе предоставлять.
              </span>
            </label>
            <p class={styles['source-selection__hint']}>
              На источник: до 1000 документов, 1 МиБ, 40 действий и 5 минут. Пустые даты — доступный
              диапазон; пустой автор — все авторы. Полнота будет указана в результате.
            </p>
            <button
              type="submit"
              disabled={
                !consent() ||
                !resource().trim() ||
                busy() ||
                props.grants.length >= 5 ||
                !props.modelProcessingAvailable
              }
            >
              {busy() ? 'Проверяем…' : 'Добавить источник'}
            </button>
          </form>
        </Show>
        <Show when={!adding() && current()}>
          {(grant) => (
            <div class={styles['source-selection__attached']}>
              <p>
                {providers[grant().provider].name} · {grant().resourceId}
              </p>
              <nav aria-label="Подключённые источники">
                <button
                  type="button"
                  disabled={busy() || selectedGrant() === 0}
                  onClick={() => setSelectedGrant(selectedGrant() - 1)}
                >
                  ←
                </button>
                <span>
                  {Math.min(selectedGrant() + 1, props.grants.length)} / {props.grants.length}
                </span>
                <button
                  type="button"
                  disabled={busy() || selectedGrant() >= props.grants.length - 1}
                  onClick={() => setSelectedGrant(selectedGrant() + 1)}
                >
                  →
                </button>
                <button type="button" disabled={busy()} onClick={() => void remove()}>
                  Убрать источник
                </button>
              </nav>
            </div>
          )}
        </Show>
        <Show when={error()}>
          <p class={styles['source-selection__error']} role="alert">
            {error()}
          </p>
        </Show>
      </Show>
      <button type="button" disabled={busy()} onClick={() => props.onClose()}>
        Готово
      </button>
    </section>
  );
}
