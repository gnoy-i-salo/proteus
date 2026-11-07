import { createEffect, createSignal, For, on, Show } from 'solid-js';
import type { StageTrace } from '@proteus-int/pipeline';
import { traceMarker } from './experiment-report';
import { classifyTraceStage } from '../model/execution-stage';
import type {
  TraceNavigation,
  TraceParticipantFilter,
  TracePhaseFilter,
} from '../model/trace-navigation';
import { TextWorkspace } from '../../../shared/ui/text-workspace/text-workspace';
import styles from './trace-reader.module.scss';

const participantOptions: readonly (readonly [TraceParticipantFilter, string])[] = [
  ['all', 'Все'],
  ['a', 'A'],
  ['b', 'B'],
  ['control', 'Контроль'],
  ['system', 'Общие'],
];
const phaseOptions: readonly (readonly [TracePhaseFilter, string])[] = [
  ['all', 'Все'],
  ['preparation', 'Подготовка'],
  ['trial', 'Прогоны'],
];
const participantTags = {
  a: 'A',
  b: 'B',
  control: 'Контроль',
  system: 'Общие',
  unknown: 'Неизвестно',
} as const;
const listPageSize = 8;
const stageTag = (item: StageTrace) => participantTags[classifyTraceStage(item).participant];
const stageLabel = (item: StageTrace) => `${stageTag(item)} · ${item.label}`;

export function TraceReader(props: { navigation: TraceNavigation; running: boolean }) {
  const navigation = () => props.navigation;
  const [windowStart, setWindowStart] = createSignal(0);
  const stages = () => navigation().stages();
  const active = () => navigation().active();
  const activeIndex = () => {
    const id = active()?.id;
    return stages().findIndex((item) => item.id === id);
  };
  const pageCount = () => Math.max(1, Math.ceil(stages().length / listPageSize));
  const visibleStages = () => stages().slice(windowStart(), windowStart() + listPageSize);
  createEffect(
    on(activeIndex, (index) => {
      if (index >= 0) setWindowStart(Math.floor(index / listPageSize) * listPageSize);
    }),
  );
  const reasoningText = (item: StageTrace) => {
    const text = item.trace.reasoning?.map((block) => block.text).join('\n\n');
    if (text) return text;
    return props.running && !item.trace.summary
      ? 'Ожидаем текст reasoning от CLI…'
      : 'CLI не передал текст reasoning для этого этапа.';
  };
  return (
    <section class={styles.traceReader} aria-label="Текст Intelligence traces">
      <Show
        when={navigation().hasStages()}
        fallback={<p class={styles.traceReader__empty}>Этапы ещё не начались.</p>}
      >
        <div class={styles.traceReader__controls}>
          <div class={styles.traceReader__filters}>
            <div class={styles.traceReader__group} role="radiogroup" aria-label="Участники">
              <For each={participantOptions}>
                {([value, label]) => (
                  <label class={styles.traceReader__option}>
                    <input
                      type="radio"
                      name="trace-participant"
                      checked={navigation().participant() === value}
                      onChange={() => navigation().setParticipant(value)}
                    />
                    {label}
                  </label>
                )}
              </For>
            </div>
            <div class={styles.traceReader__group} role="radiogroup" aria-label="Фазы">
              <For each={phaseOptions}>
                {([value, label]) => (
                  <label class={styles.traceReader__option}>
                    <input
                      type="radio"
                      name="trace-phase"
                      checked={navigation().phase() === value}
                      onChange={() => navigation().setPhase(value)}
                    />
                    {label}
                  </label>
                )}
              </For>
            </div>
            <Show when={navigation().phase() === 'trial'}>
              <label class={styles.traceReader__option}>
                Прогон
                <select
                  aria-label="Прогон"
                  value={navigation().round() ?? ''}
                  onChange={(event) =>
                    navigation().setRound(
                      event.currentTarget.value === '' ? null : Number(event.currentTarget.value),
                    )
                  }
                >
                  <option value="">Все</option>
                  <For each={navigation().availableRounds()}>
                    {(round) => <option value={round}>{round}</option>}
                  </For>
                </select>
              </label>
            </Show>
          </div>
          <Show when={props.running}>
            <div class={styles.traceReader__follow}>
              <Show
                when={navigation().following()}
                fallback={
                  <>
                    <span class={styles.traceReader__followState}>Следование приостановлено</span>
                    <button type="button" onClick={() => navigation().resume()}>
                      К текущему этапу
                    </button>
                  </>
                }
              >
                <span class={styles.traceReader__followState} data-active="true">
                  Следим за текущим этапом
                </span>
              </Show>
            </div>
          </Show>
        </div>
        <Show
          when={stages().length > 0}
          fallback={<p class={styles.traceReader__empty}>Нет этапов для выбранных фильтров.</p>}
        >
          <div class={styles.traceReader__body}>
            <div class={styles.traceReader__stages}>
              <select
                class={styles.traceReader__stageSelect}
                aria-label="Этап"
                value={active()?.id ?? ''}
                onChange={(event) => navigation().select(event.currentTarget.value)}
              >
                <For each={stages()}>
                  {(item) => <option value={item.id}>{stageLabel(item)}</option>}
                </For>
              </select>
              <ul class={styles.traceReader__stageList}>
                <For each={visibleStages()}>
                  {(item) => (
                    <li>
                      <button
                        type="button"
                        class={styles.traceReader__stageItem}
                        classList={{
                          [styles.traceReader__stageItem_active!]: active()?.id === item.id,
                        }}
                        aria-pressed={active()?.id === item.id}
                        aria-label={stageLabel(item)}
                        onClick={() => navigation().select(item.id)}
                      >
                        <span
                          class={styles.traceReader__stageTag}
                          data-participant={classifyTraceStage(item).participant}
                        >
                          {stageTag(item)}
                        </span>
                        <strong class={styles.traceReader__stageName}>{item.label}</strong>
                        <small class={styles.traceReader__stageMeta}>
                          {item.trace.provider} ·{' '}
                          {((item.trace.wallTimeMs ?? item.trace.reasoningMs) / 1000).toFixed(1)} с
                          · {traceMarker(item, props.running)}
                        </small>
                      </button>
                    </li>
                  )}
                </For>
              </ul>
              <Show when={stages().length > listPageSize}>
                <nav class={styles.traceReader__stagePages} aria-label="Страницы списка этапов">
                  <button
                    type="button"
                    aria-label="Предыдущая страница этапов"
                    disabled={windowStart() === 0}
                    onClick={() => {
                      navigation().pause();
                      setWindowStart(Math.max(0, windowStart() - listPageSize));
                    }}
                  >
                    ←
                  </button>
                  <span>
                    {Math.floor(windowStart() / listPageSize) + 1} / {pageCount()}
                  </span>
                  <button
                    type="button"
                    aria-label="Следующая страница этапов"
                    disabled={windowStart() + listPageSize >= stages().length}
                    onClick={() => {
                      navigation().pause();
                      setWindowStart(windowStart() + listPageSize);
                    }}
                  >
                    →
                  </button>
                </nav>
              </Show>
            </div>
            <div class={styles.traceReader__main}>
              <Show
                when={active()?.id}
                fallback={<p class={styles.traceReader__empty}>Выберите этап в списке.</p>}
              >
                {(() => {
                  const item = () => active()!;
                  return (
                    <>
                      <header class={styles.traceReader__readerHead}>
                        <strong class={styles.traceReader__readerTitle}>{item().label}</strong>
                        <span class={styles.traceReader__readerMeta}>
                          {item().trace.provider} ·{' '}
                          {((item().trace.wallTimeMs ?? item().trace.reasoningMs) / 1000).toFixed(
                            1,
                          )}{' '}
                          с · {traceMarker(item(), props.running)}
                        </span>
                      </header>
                      <Show when={item().trace.reasoningTruncated}>
                        <p class={styles.traceReader__badge}>
                          Текст reasoning сокращён до лимита сохранения.
                        </p>
                      </Show>
                      <Show when={item().trace.summary.length > 200}>
                        <details
                          class={styles.traceReader__summary}
                          onToggle={(event) => {
                            if (event.currentTarget.open) navigation().pause();
                          }}
                        >
                          <summary>Сводка этапа — открыть целиком</summary>
                          <div class={styles.traceReader__summaryReader}>
                            <TextWorkspace
                              label="Сводка этапа"
                              value={item().trace.summary}
                              resetKey={item().id}
                              position={() => navigation().positionFor(item().id, 'summary')}
                              onPositionChange={(position) =>
                                navigation().savePosition(item().id, position, 'summary')
                              }
                              onInteraction={() => navigation().pause()}
                            />
                          </div>
                        </details>
                      </Show>
                      <div class={styles.traceReader__reader}>
                        <TextWorkspace
                          label="Текст"
                          value={reasoningText(item())}
                          resetKey={item().id}
                          position={() => navigation().positionFor(item().id)}
                          onPositionChange={(position) =>
                            navigation().savePosition(item().id, position)
                          }
                          onInteraction={() => navigation().pause()}
                        />
                      </div>
                    </>
                  );
                })()}
              </Show>
            </div>
          </div>
        </Show>
      </Show>
    </section>
  );
}
