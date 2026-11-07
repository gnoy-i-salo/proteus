import { createSignal, createEffect, on, onCleanup, onMount } from 'solid-js';
import { paginateInput, locateTextOffset } from '../../../shared/lib/pagination';
import styles from './experiment-page.module.scss';

export function PagedField(props: {
  label: string;
  pages: string[];
  onChange: (pages: string[]) => void;
  compact?: boolean;
  revision: number;
}) {
  const [page, setPage] = createSignal(0);
  let editor: HTMLTextAreaElement | undefined;
  const reflow = (value: string, replaceAll = false, preservePosition = false) => {
    if (!editor) return;
    const css = getComputedStyle(editor);
    const canvas = document.createElement('canvas');
    const context = canvas.getContext('2d');
    if (context) context.font = css.font;
    const characterWidth = context?.measureText('M').width || parseFloat(css.fontSize) * 0.61;
    const horizontalPadding = parseFloat(css.paddingLeft) + parseFloat(css.paddingRight);
    const verticalPadding = parseFloat(css.paddingTop) + parseFloat(css.paddingBottom);
    const chunks = paginateInput(
      value,
      Math.max(1, Math.floor((editor.clientWidth - horizontalPadding) / characterWidth)),
      Math.max(1, Math.floor((editor.clientHeight - verticalPadding) / parseFloat(css.lineHeight))),
    );
    const index = replaceAll ? 0 : page();
    const focused = document.activeElement === editor;
    const oldOffset = props.pages.slice(0, page()).join('').length;
    const selectionStart = oldOffset + (focused ? editor.selectionStart : 0);
    const selectionEnd = oldOffset + (focused ? editor.selectionEnd : 0);
    const location = locateTextOffset(chunks, selectionStart);
    props.onChange(
      replaceAll
        ? chunks
        : [...props.pages.slice(0, index), ...chunks, ...props.pages.slice(index + 1)],
    );
    setPage(preservePosition ? location.page : replaceAll ? 0 : index + chunks.length - 1);
    if (preservePosition && focused) {
      queueMicrotask(() => {
        editor?.setSelectionRange(location.offset, location.offset + selectionEnd - selectionStart);
      });
    }
  };
  createEffect(
    on(
      () => props.revision,
      () => {
        reflow(props.pages.join(''), true);
      },
    ),
  );
  onMount(() => {
    reflow(props.pages.join(''), true);
    let dimensions = `${editor?.clientWidth}:${editor?.clientHeight}`;
    const observer = new ResizeObserver(() => {
      const next = `${editor?.clientWidth}:${editor?.clientHeight}`;
      if (next !== dimensions) reflow(props.pages.join(''), true, true);
      dimensions = next;
    });
    if (editor) observer.observe(editor);
    onCleanup(() => observer.disconnect());
  });
  return (
    <label class={styles['experiment__field']} data-compact={props.compact ? 'true' : 'false'}>
      <span>{props.label}</span>
      <textarea
        aria-label={props.label}
        ref={(element) => {
          editor = element;
        }}
        maxLength={4000}
        value={props.pages[page()] ?? ''}
        placeholder="Опишите своими словами…"
        onInput={(event) => reflow(event.currentTarget.value)}
      />
      <span class={styles['experiment__pager']}>
        <button
          type="button"
          aria-label={`Назад: ${props.label}`}
          disabled={page() === 0}
          onClick={() => setPage(page() - 1)}
        >
          ←
        </button>
        <small>
          {page() + 1}/{props.pages.length}
        </small>
        <button
          type="button"
          aria-label={`Далее: ${props.label}`}
          disabled={page() === props.pages.length - 1 && props.pages.length >= 35}
          onClick={() => {
            if (page() === props.pages.length - 1) props.onChange([...props.pages, '']);
            setPage(page() + 1);
          }}
        >
          →
        </button>
      </span>
    </label>
  );
}
