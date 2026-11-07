import { createSignal, createMemo, createEffect, on, For, Show } from 'solid-js';
import type { StageTrace } from '@proteus-int/pipeline';
import { traceMarker } from './experiment-report';
import { PagedText } from './paged-text';
import styles from './experiment-page.module.scss';

export function TraceReader(props: { stages: StageTrace[]; running: boolean; person: number }) {
  const [chosen, setChosen] = createSignal('');
  const [page, setPage] = createSignal(0);
  const [pinned, setPinned] = createSignal(false);
  const stages = () =>
    props.stages.filter(
      (item) => item.person === props.person || item.person === null || item.person === 2,
    );
  const keys = createMemo(() => stages().map((item) => item.id));
  const visible = () => keys().slice(page() * 3, page() * 3 + 3);
  const active = () => (visible().includes(chosen()) ? chosen() : visible()[0]);
  const selectPage = (next: number) => {
    setPinned(true);
    setPage(next);
    setChosen(keys()[next * 3] ?? '');
  };
  createEffect(
    on(
      () => props.person,
      () => {
        setChosen('');
        setPage(0);
        setPinned(false);
      },
    ),
  );
  createEffect(() => {
    const ids = keys();
    if (props.running && !pinned() && ids.length) {
      setChosen(ids.at(-1)!);
      setPage(Math.floor((ids.length - 1) / 3));
    } else if (!chosen() && ids.length) setChosen(ids[0]!);
  });
  return (
    <section class={styles['experiment__reasoning']} aria-label="Текст Intelligence traces">
      <Show when={keys().length} fallback={<p>Этапы этой персоны ещё не начались.</p>}>
        <For each={visible()}>
          {(id) => {
            const item = () => stages().find((stage) => stage.id === id)!;
            const label = () =>
              `${item().person === null ? 'SYS' : item().person === 2 ? 'CTRL' : item().person === 0 ? 'A' : 'B'} · ${item().label}`;
            const text = () =>
              item()
                .trace.reasoning?.map((block) => block.text)
                .join('\n\n') ||
              (props.running && !item().trace.summary
                ? 'Ожидаем текст reasoning от CLI…'
                : 'CLI не передал текст reasoning для этого этапа.');
            return (
              <article class={styles['experiment__trace']} data-expanded={active() === id}>
                <button
                  class={styles['experiment__trace-heading']}
                  aria-label={label()}
                  aria-expanded={active() === id}
                  onClick={() => {
                    setPinned(true);
                    setChosen(id);
                  }}
                >
                  <strong>{label()}</strong>
                  <small>
                    {item().trace.provider} ·{' '}
                    {((item().trace.wallTimeMs ?? item().trace.reasoningMs) / 1000).toFixed(1)} с
                  </small>
                  <span>{traceMarker(item(), props.running)}</span>
                </button>
                <Show when={active() === id}>
                  <p class={styles['experiment__reasoning-note']}>
                    {item().trace.summary || (props.running ? 'Выполняется…' : 'Этап не завершён')}
                    {item().trace.reasoningTruncated
                      ? ' · Текст сокращён до лимита сохранения.'
                      : ''}
                  </p>
                  <PagedText text={text()} resetKey={id} onInteraction={() => setPinned(true)} />
                </Show>
              </article>
            );
          }}
        </For>
        <nav class={styles['experiment__stage-pages']} aria-label="Страницы этапов">
          <button disabled={page() === 0} onClick={() => selectPage(page() - 1)}>
            ←
          </button>
          <span>
            {page() + 1} / {Math.ceil(keys().length / 3)}
          </span>
          <button
            disabled={(page() + 1) * 3 >= keys().length}
            onClick={() => selectPage(page() + 1)}
          >
            →
          </button>
        </nav>
      </Show>
    </section>
  );
}
