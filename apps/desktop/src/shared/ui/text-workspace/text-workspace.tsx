import { createEffect, createSignal, For, onCleanup, onMount, Show } from 'solid-js';
import {
  domOffset,
  moveTextOffset,
  selectedCharacterCount,
  sourceOffset,
} from '../../lib/text-document';
import { createTextWorkspace } from './text-workspace-model';
import styles from './text-workspace.module.scss';

export function TextWorkspace(props: {
  readonly label: string;
  readonly value: string;
  readonly onChange?: (value: string) => void;
  readonly resetKey?: string | number;
  readonly compact?: boolean | undefined;
  readonly onInteraction?: (() => void) | undefined;
}) {
  const model = createTextWorkspace({
    value: () => props.value,
    resetKey: () => props.resetKey,
    change: (value) => props.onChange?.(value),
    interact: () => props.onInteraction?.(),
  });
  const [expanded, setExpanded] = createSignal(false);
  let frame!: HTMLElement;
  let content!: HTMLElement;
  let fontProbe!: HTMLSpanElement;
  let editor: HTMLTextAreaElement | undefined;
  let toggle!: HTMLButtonElement;
  let restoreInert: (() => void) | undefined;
  let focused = false;
  const restoreSelection = () => {
    if (!editor) return;
    const range = model.projected();
    editor.setSelectionRange(
      domOffset(model.visible(), range.start),
      domOffset(model.visible(), range.end),
      range.direction,
    );
    // Each rendered page fits. No implicit scroll is used to reveal a clipped selection.
    editor.scrollTop = 0;
    editor.scrollLeft = 0;
  };
  const captureSelection = () => {
    if (!editor) return;
    const current = model.projected();
    const start = sourceOffset(model.visible(), editor.selectionStart);
    const end = sourceOffset(model.visible(), editor.selectionEnd);
    if (
      current.continues &&
      start === current.start &&
      end === current.end &&
      editor.selectionDirection === current.direction
    )
      return;
    model.select({
      start: model.page().start + start,
      end: model.page().start + end,
      direction: editor.selectionDirection,
    });
  };
  const measure = () => {
    const css = getComputedStyle(content);
    const canvas = document.createElement('canvas');
    const context = canvas.getContext('2d');
    if (context) context.font = css.font;
    const width =
      content.clientWidth - parseFloat(css.paddingLeft) - parseFloat(css.paddingRight) - 2;
    const height =
      content.clientHeight - parseFloat(css.paddingTop) - parseFloat(css.paddingBottom);
    if (width <= 0 || height <= 0) return;
    model.setLayout({
      width,
      rows: Math.max(1, Math.floor(height / parseFloat(css.lineHeight))),
      measure: (text) => context?.measureText(text).width ?? text.length * 9,
    });
    queueMicrotask(restoreSelection);
  };
  const setExpandedMode = (next: boolean) => {
    captureSelection();
    focused = document.activeElement === editor;
    setExpanded(next);
    props.onInteraction?.();
    if (next) {
      const siblings: { element: HTMLElement; inert: boolean }[] = [];
      let current: HTMLElement | null = frame;
      while (current?.parentElement && current.parentElement !== document.body) {
        for (const sibling of current.parentElement.children) {
          if (sibling !== current && sibling instanceof HTMLElement) {
            siblings.push({ element: sibling, inert: sibling.inert });
            sibling.inert = true;
          }
        }
        current = current.parentElement;
      }
      restoreInert = () => {
        for (const item of siblings) item.element.inert = item.inert;
      };
    } else {
      restoreInert?.();
      restoreInert = undefined;
    }
    queueMicrotask(() => {
      measure();
      if (focused && editor) editor.focus();
      else toggle.focus();
      restoreSelection();
    });
  };
  onMount(() => {
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(content);
    observer.observe(fontProbe);
    onCleanup(() => {
      observer.disconnect();
      restoreInert?.();
    });
  });
  createEffect(() => {
    model.visible();
    model.selection();
    queueMicrotask(restoreSelection);
  });
  const navigate = (next: number) => {
    model.navigate(next);
    queueMicrotask(restoreSelection);
  };
  const replace = (text: string) => {
    model.replace(text);
    queueMicrotask(restoreSelection);
  };
  const copy = (event: ClipboardEvent, cut: boolean) => {
    if (!event.clipboardData) return;
    captureSelection();
    if (model.selection().start === model.selection().end) return;
    event.preventDefault();
    event.clipboardData?.setData('text/plain', model.selectedText());
    if (cut) replace('');
  };
  return (
    <section
      ref={(element) => {
        frame = element;
      }}
      class={styles['textWorkspace']}
      classList={{
        [styles['textWorkspace_expanded']!]: expanded(),
        [styles['textWorkspace_compact']!]: !!props.compact,
        [styles['textWorkspace_reader']!]: !props.onChange,
      }}
      role={expanded() ? 'dialog' : undefined}
      aria-modal={expanded() ? 'true' : undefined}
      aria-label={expanded() ? props.label : undefined}
      onKeyDown={(event) => {
        if (!expanded()) return;
        if (event.key === 'Escape') {
          event.preventDefault();
          event.stopPropagation();
          setExpandedMode(false);
        }
        if (event.key === 'Tab') {
          const controls = [
            ...frame.querySelectorAll<HTMLElement>(
              'button:not(:disabled), input, select, textarea, [tabindex="0"]',
            ),
          ];
          const first = controls[0];
          const last = controls.at(-1);
          if (event.shiftKey && document.activeElement === first) {
            event.preventDefault();
            last?.focus();
          } else if (!event.shiftKey && document.activeElement === last) {
            event.preventDefault();
            first?.focus();
          }
        }
      }}
    >
      <span
        ref={(element) => {
          fontProbe = element;
        }}
        class={styles['textWorkspace__fontProbe']}
        aria-hidden="true"
      />
      <header class={styles['textWorkspace__heading']}>
        <Show
          when={model.projected().continues && model.selection().end > model.selection().start}
          fallback={<span>{props.label}</span>}
        >
          <span
            class={styles['textWorkspace__selection']}
            role="status"
            title={`Выделено ${selectedCharacterCount(model.selectedText())} символов, включая другие страницы. Копирование и замена применяются ко всему выделению.`}
          >
            {selectedCharacterCount(model.selectedText())} симв. ↔
          </span>
        </Show>
        <button
          ref={(element) => {
            toggle = element;
          }}
          type="button"
          aria-label={`${expanded() ? 'Свернуть' : 'Развернуть'}: ${props.label}`}
          onClick={() => setExpandedMode(!expanded())}
        >
          {expanded() ? '↙' : '↗'}
        </button>
      </header>
      <Show when={expanded()}>
        <div class={styles['textWorkspace__tools']}>
          <label>
            Найти точно
            <input
              aria-label={`Поиск: ${props.label}`}
              value={model.query()}
              onInput={(event) => {
                model.search(event.currentTarget.value);
                model.find();
              }}
              onKeyDown={(event) => {
                if (event.key === 'Enter') {
                  event.preventDefault();
                  model.find(event.shiftKey ? -1 : 1);
                }
              }}
            />
          </label>
          <button type="button" disabled={!model.matches().length} onClick={() => model.find()}>
            Следующее совпадение
          </button>
          <span role="status">
            {model.query() ? `${model.match() + 1} / ${model.matches().length}` : ''}
          </span>
          <button
            type="button"
            disabled={!model.bookmark()}
            onClick={() => {
              model.returnToPlace();
              queueMicrotask(() => {
                content.focus();
                restoreSelection();
              });
            }}
          >
            Вернуться к месту
          </button>
          <label>
            Раздел
            <select
              aria-label={`Оглавление: ${props.label}`}
              value={String(
                model
                  .outline()
                  .filter((section) => section.offset <= model.anchor())
                  .at(-1)?.offset ?? 0,
              )}
              onChange={(event) => {
                const offset = Number(event.currentTarget.value);
                model.setAnchor(offset);
                model.select({ start: offset, end: offset, direction: 'none' });
                props.onInteraction?.();
              }}
            >
              <For each={model.outline()}>
                {(section, index) => (
                  <option value={section.offset}>
                    {index() + 1}. {section.label}
                  </option>
                )}
              </For>
            </select>
          </label>
        </div>
      </Show>
      <Show
        when={props.onChange}
        fallback={
          <div
            ref={(element) => {
              content = element;
            }}
            class={styles['textWorkspace__content']}
            tabindex="0"
            aria-label={props.label}
          >
            <p>
              <Show
                when={model.query() && model.match() >= 0}
                fallback={model.visible() || 'Пока нет данных.'}
              >
                {model.visible().slice(0, model.projected().start)}
                <mark>{model.visible().slice(model.projected().start, model.projected().end)}</mark>
                {model.visible().slice(model.projected().end)}
              </Show>
            </p>
          </div>
        }
      >
        <textarea
          ref={(element) => {
            editor = element;
            content = element;
          }}
          class={styles['textWorkspace__content']}
          aria-label={props.label}
          placeholder="Опишите своими словами…"
          value={model.visible()}
          onSelect={captureSelection}
          onKeyUp={captureSelection}
          onClick={captureSelection}
          onInput={(event) => {
            model.editPage(
              event.currentTarget.value,
              event.currentTarget.selectionStart,
              event.currentTarget.selectionEnd,
              event.currentTarget.selectionDirection,
            );
            queueMicrotask(restoreSelection);
          }}
          onBeforeInput={(event) => {
            captureSelection();
            if (!model.projected().continues) return;
            const input = event;
            if (input.inputType === 'insertText' && input.data !== null) {
              event.preventDefault();
              replace(input.data);
            } else if (
              input.inputType === 'insertLineBreak' ||
              input.inputType === 'insertParagraph'
            ) {
              event.preventDefault();
              replace('\n');
            } else if (input.inputType.startsWith('delete')) {
              event.preventDefault();
              replace('');
            }
          }}
          onPaste={(event) => {
            if (!event.clipboardData) return;
            event.preventDefault();
            captureSelection();
            replace(event.clipboardData?.getData('text/plain') ?? '');
          }}
          onCopy={(event) => copy(event, false)}
          onCut={(event) => copy(event, true)}
          onKeyDown={(event) => {
            if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'a') {
              event.preventDefault();
              model.select({ start: 0, end: props.value.length, direction: 'forward' });
              return;
            }
            if (
              !event.shiftKey &&
              model.projected().continues &&
              ['ArrowLeft', 'ArrowRight'].includes(event.key)
            ) {
              event.preventDefault();
              const offset =
                event.key === 'ArrowLeft' ? model.selection().start : model.selection().end;
              model.select({ start: offset, end: offset, direction: 'none' }, true);
              return;
            }
            if (event.shiftKey && ['ArrowLeft', 'ArrowRight'].includes(event.key)) {
              captureSelection();
              const range = model.selection();
              const backwards = range.direction === 'backward';
              const anchor = backwards ? range.end : range.start;
              const focus = backwards ? range.start : range.end;
              const next = moveTextOffset(props.value, focus, event.key === 'ArrowRight' ? 1 : -1);
              if (
                next < model.page().start ||
                next > model.page().end ||
                model.projected().continues
              ) {
                event.preventDefault();
                model.select(
                  {
                    start: Math.min(anchor, next),
                    end: Math.max(anchor, next),
                    direction: next < anchor ? 'backward' : 'forward',
                  },
                  true,
                );
              }
            }
          }}
        />
      </Show>
      <footer class={styles['textWorkspace__footer']}>
        <nav aria-label={props.onChange ? `Страницы: ${props.label}` : 'Страницы текста'}>
          <button
            type="button"
            aria-label={props.onChange ? `Назад: ${props.label}` : 'Предыдущая страница текста'}
            title="Предыдущая страница"
            disabled={model.pageIndex() === 0}
            onClick={() => navigate(model.pageIndex() - 1)}
          >
            ←
          </button>
          <span title={`Страница ${model.pageIndex() + 1} из ${model.pages().length}`}>
            {model.pageIndex() + 1} / {model.pages().length}
          </span>
          <button
            type="button"
            aria-label={props.onChange ? `Далее: ${props.label}` : 'Следующая страница текста'}
            title="Следующая страница"
            disabled={model.pageIndex() >= model.pages().length - 1}
            onClick={() => navigate(model.pageIndex() + 1)}
          >
            →
          </button>
        </nav>
      </footer>
    </section>
  );
}
