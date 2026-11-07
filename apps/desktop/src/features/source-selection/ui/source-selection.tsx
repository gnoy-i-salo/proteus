import { createSignal, For, onCleanup, onMount, Show, untrack } from 'solid-js';
import { connectionProviders } from '@proteus-int/contracts';
import type { ImportedSource, SourceGrant, SourceProvider } from '@proteus-int/contracts';
import {
  deleteSourceGrant,
  createSourceGrant,
  importSource,
} from '../../../shared/lib/connections';
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
  const [provider, setProvider] = createSignal<SourceProvider>('telegram');
  const [resource, setResource] = createSignal('');
  const [consent, setConsent] = createSignal(false);
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal('');
  const [selectedGrant, setSelectedGrant] = createSignal(0);
  const [adding, setAdding] = createSignal(untrack(() => props.grants.length === 0));
  let dialog: HTMLElement | undefined;
  let disposed = false;
  const previousFocus = document.activeElement;
  const current = () => props.grants[Math.min(selectedGrant(), props.grants.length - 1)];
  onMount(() => dialog?.querySelector<HTMLButtonElement>('button')?.focus());
  onCleanup(() => {
    disposed = true;
    if (previousFocus instanceof HTMLElement) previousFocus.focus();
  });
  const pick = async () => {
    setBusy(true);
    setError('');
    try {
      const source = await importSource(provider());
      if (source && !disposed) {
        props.onImported(provider(), source);
        setResource(source.id);
        setConsent(false);
      }
    } catch {
      if (!disposed)
        setError('Не удалось прочитать файл. Выберите поддерживаемый JSON или текстовый экспорт.');
    } finally {
      if (!disposed) setBusy(false);
    }
  };
  const add = async (form: HTMLFormElement) => {
    if (!consent() || busy() || props.grants.length >= 5 || !props.modelProcessingAvailable) return;
    const fields = new FormData(form);
    const date = (key: string, end: boolean) => {
      const value = String(fields.get(key) ?? '');
      return value ? `${value}T${end ? '23:59:59.999' : '00:00:00.000'}Z` : null;
    };
    setBusy(true);
    setError('');
    try {
      const grant = await createSourceGrant({
        provider: provider(),
        resourceId: resource().trim(),
        authorId: String(fields.get('author') ?? '').trim() || null,
        from: date('from', false),
        to: date('to', true),
        allowModelProcessing: true,
        operations: ['describe', 'search', 'collect', 'read', 'coverage'],
        budget: {
          maxSteps: 40,
          maxDocuments: 1000,
          maxBytes: 1048576,
          maxDurationMs: 300000,
        },
      });
      if (!disposed) {
        props.onChange([...props.grants, grant]);
        setSelectedGrant(props.grants.length - 1);
        setAdding(false);
        setConsent(false);
      }
    } catch {
      if (!disposed)
        setError(
          'Не удалось разрешить доступ. Проверьте подключение, ресурс, автора и диапазон дат.',
        );
    } finally {
      if (!disposed) setBusy(false);
    }
  };
  const remove = async () => {
    const grant = current();
    if (!grant) return;
    setBusy(true);
    setError('');
    try {
      await deleteSourceGrant(grant.id);
      if (!disposed) {
        props.onChange(props.grants.filter((item) => item.id !== grant.id));
        if (!props.grants.length) setAdding(true);
      }
    } catch {
      if (!disposed) setError('Не удалось отменить доступ. Повторите попытку.');
    } finally {
      if (!disposed) setBusy(false);
    }
  };
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
              void add(event.currentTarget);
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
                    setProvider(event.currentTarget.value as SourceProvider);
                    setResource('');
                    setConsent(false);
                    setError('');
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
                      setResource(event.currentTarget.value);
                      setConsent(false);
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
                    setResource(event.currentTarget.value);
                    setConsent(false);
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
