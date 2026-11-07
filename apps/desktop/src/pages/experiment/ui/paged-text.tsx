import { createSignal, createMemo, createEffect, on, onCleanup, onMount } from 'solid-js';
import { paginateText } from '../../../shared/lib/pagination';
import styles from './experiment-page.module.scss';

export function PagedText(props: { text: string; resetKey?: string; onInteraction?: () => void }) {
  const [page, setPage] = createSignal(0);
  const [size, setSize] = createSignal({ columns: 15, rows: 1 });
  let reading!: HTMLDivElement;
  onMount(() => {
    const measure = () => {
      const paragraph = reading.querySelector('p');
      if (!paragraph) return;
      const css = getComputedStyle(paragraph);
      const frame = getComputedStyle(reading);
      const width =
        reading.clientWidth - parseFloat(frame.paddingLeft) - parseFloat(frame.paddingRight);
      const height =
        reading.clientHeight - parseFloat(frame.paddingTop) - parseFloat(frame.paddingBottom) - 48;
      setSize({
        columns: Math.max(15, Math.floor(width / (parseFloat(css.fontSize) * 0.61))),
        rows: Math.max(1, Math.floor(height / parseFloat(css.lineHeight))),
      });
    };
    // Size the first page before the asynchronous ResizeObserver notification.
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(reading);
    onCleanup(() => observer.disconnect());
  });
  const chunks = createMemo(() =>
    paginateText(props.text || 'Пока нет данных.', size().columns, size().rows),
  );
  createEffect(
    on(
      () => [props.resetKey ?? props.text, size()],
      () => setPage(0),
    ),
  );
  return (
    <div
      ref={(element) => {
        reading = element;
      }}
      class={styles['experiment__reading']}
    >
      <p>{chunks()[page()]}</p>
      <nav aria-label="Страницы текста">
        <button
          disabled={page() === 0}
          onClick={() => {
            props.onInteraction?.();
            setPage(page() - 1);
          }}
        >
          ←
        </button>
        <span>
          {page() + 1} / {chunks().length}
        </span>
        <button
          disabled={page() >= chunks().length - 1}
          onClick={() => {
            props.onInteraction?.();
            setPage(page() + 1);
          }}
        >
          →
        </button>
      </nav>
    </div>
  );
}
