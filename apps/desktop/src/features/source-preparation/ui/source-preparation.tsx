import { createSignal, For, onCleanup, onMount, Show } from 'solid-js';
import { isDesktop } from '../../../shared/lib/window-controls';
import { createSourcePreparation } from '../model/source-preparation';
import type { SourceFileKind } from '../model/source-preview';
import { SourceImportForm } from './source-import-form';
import { SourceImportProgress } from './source-import-progress';
import styles from './source-preparation.module.scss';
const formats: readonly { id: SourceFileKind; label: string }[] = [
  { id: 'telegramArchive', label: 'Telegram Desktop JSON' },
  { id: 'savedHtml', label: 'Сохранённая HTML-страница' },
  { id: 'savedText', label: 'Текстовый файл' },
  { id: 'savedMarkdown', label: 'Markdown' },
];
export function SourcePreparation(props: {
  readonly letter: string;
  readonly personaId: string;
  readonly onClose: () => void;
  readonly onCleanupError: (error: unknown) => void;
}) {
  const model = createSourcePreparation({
    personaId: () => props.personaId,
    onCleanupError: (cause) => props.onCleanupError(cause),
  });
  const [format, setFormat] = createSignal<SourceFileKind>('telegramArchive');
  const [url, setUrl] = createSignal('');
  let dialog: HTMLElement | undefined;
  const previousFocus = document.activeElement;
  onMount(() => dialog?.querySelector<HTMLButtonElement>('button')?.focus());
  onCleanup(() => {
    if (previousFocus instanceof HTMLElement) previousFocus.focus();
  });
  return (
    <section
      ref={(element) => {
        dialog = element;
      }}
      class={styles.sourcePreparation}
      role="dialog"
      aria-modal="true"
      aria-label={`Подготовка источника ${props.letter}`}
      onKeyDown={(event) => {
        if (event.key === 'Escape') props.onClose();
        if (event.key !== 'Tab' || !dialog) return;
        const targets = [...dialog.querySelectorAll<HTMLElement>('button, input, select')].filter(
          (item) => !item.matches(':disabled') && item.offsetParent !== null,
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
        <h2>Подготовка источника {props.letter}</h2>
        <button
          type="button"
          aria-label="Закрыть подготовку источника"
          onClick={() => props.onClose()}
        >
          ×
        </button>
      </header>
      <Show
        when={isDesktop()}
        fallback={<p>Подготовка источников доступна в настольном приложении PROTEUS.</p>}
      >
        <Show when={!model.preview.preview() && !model.ingestion.job()}>
          <p>
            Лимиты: файл до 128 МиБ, текст до 256 МиБ, до 100 000 документов, 60 секунд на операцию.
          </p>
        </Show>
        <Show when={!model.ingestion.job()}>
          <Show when={!model.preview.preview()}>
            <div class={styles.sourcePreparation__actions}>
              <label>
                Формат файла
                <select
                  disabled={model.locked() || model.preview.busy()}
                  value={format()}
                  onChange={(event) => {
                    const selected = formats.find((item) => item.id === event.currentTarget.value);
                    if (selected) setFormat(selected.id);
                  }}
                >
                  <For each={formats}>
                    {(item) => <option value={item.id}>{item.label}</option>}
                  </For>
                </select>
              </label>
              <button
                disabled={model.locked() || model.preview.busy()}
                onClick={() => void model.choose(format()).catch(model.reportError)}
              >
                Выбрать файл
              </button>
            </div>
            <form
              onSubmit={(event) => {
                event.preventDefault();
                void model.previewUrl(url()).catch(model.reportError);
              }}
            >
              <label>
                Публичная ссылка
                <input
                  type="url"
                  required
                  value={url()}
                  disabled={model.locked() || model.preview.busy()}
                  onInput={(event) => setUrl(event.currentTarget.value)}
                />
              </label>
              <button disabled={model.locked() || model.preview.busy()}>Прочитать страницу</button>
            </form>
            <Show when={model.preview.busy()}>
              <p role="status">Читаю сведения об источнике…</p>
              <button onClick={() => model.preview.cancel()}>Отменить чтение</button>
            </Show>
          </Show>
          <Show when={model.preview.selected()}>
            {(selected) => <p class={styles.sourcePreparation__exact}>{selected().label}</p>}
          </Show>
          <Show when={model.preview.preview()} keyed>
            {(preview) => (
              <>
                <button
                  type="button"
                  disabled={model.ingestion.busy()}
                  onClick={() => model.preview.cancel()}
                >
                  Выбрать другой источник
                </button>
                <SourceImportForm
                  preview={preview}
                  busy={model.preview.busy() || model.ingestion.busy()}
                  onStart={model.start}
                  onError={model.reportError}
                />
              </>
            )}
          </Show>
        </Show>
        <SourceImportProgress model={model.ingestion} />
        <Show when={model.error() || model.preview.error() || model.ingestion.error()}>
          <p role="alert">{model.error() || model.preview.error() || model.ingestion.error()}</p>
        </Show>
        <Show
          when={
            model.ingestion.job() &&
            ['running', 'queued', 'paused'].includes(model.ingestion.job()!.status)
          }
        >
          <p>Закрытие окна остановит незавершённый импорт.</p>
        </Show>
      </Show>
    </section>
  );
}
