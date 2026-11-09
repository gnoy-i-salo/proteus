import { createSignal, For } from 'solid-js';
import type { LegacyV1Result, StoredV4Result } from '@proteus-int/pipeline';
import { createTraceNavigation } from '../model/trace-navigation';
import { ExperimentResultView } from './experiment-result-view';
import { PagedText } from './paged-text';
import styles from './experiment-history.module.scss';

export function HistorySeries(props: { readonly result: StoredV4Result }) {
  const [selected, setSelected] = createSignal(0);
  const [detail, setDetail] = createSignal('analysis');
  const traces = createTraceNavigation({
    stages: () => props.result.traces,
    running: () => false,
    activeStageId: () => undefined,
    failedStageId: () => undefined,
  });
  return (
    <div
      class={styles.experimentHistory__series}
      data-palette={selected() === 1 ? 'violet' : 'turquoise'}
    >
      <nav class={styles.experimentHistory__tabs} aria-label="Участники сохранённого запуска">
        <For
          each={[
            ...props.result.people.map((person) => person.profile.label),
            'Контроль',
            'Сравнение',
          ]}
        >
          {(label, index) => (
            <button
              type="button"
              aria-pressed={selected() === index()}
              onClick={() => setSelected(index())}
            >
              {label}
            </button>
          )}
        </For>
      </nav>
      <section class={styles.experimentHistory__result}>
        <ExperimentResultView
          readOnly
          run={{
            result: () => props.result,
            failureReason: () => props.result.trials.find((trial) => trial.error)?.error ?? '',
          }}
          traces={traces}
          selected={selected()}
          detail={detail()}
          onDetail={setDetail}
        />
      </section>
    </div>
  );
}

export function HistoryOriginal(props: { readonly result: LegacyV1Result }) {
  const [selected, setSelected] = createSignal(0);
  const [detail, setDetail] = createSignal('answer');
  const person = () => props.result.people[selected()]!;
  const text = () => {
    const value = person();
    if (detail() === 'answer') return value.attempt.answer;
    if (detail() === 'profile')
      return [value.profile.archetype, ...value.profile.skills].join('\n\n');
    if (detail() === 'sources')
      return [
        value.report.character,
        value.report.thinkingStyle,
        value.report.activity,
        value.report.behaviorSummary,
        ...value.report.evidence.map(
          (item) => `${item.id} · ${item.kind}\n${item.sourceUrl}\n${item.summary}`,
        ),
      ].join('\n\n');
    if (detail() === 'input') return props.result.input.task;
    return [
      props.result.comparison.summary,
      ...props.result.comparison.keyDifferences,
      ...props.result.comparison.people[selected()]!.observations,
    ].join('\n\n');
  };
  return (
    <section
      class={styles.experimentHistory__original}
      data-palette={selected() === 1 ? 'violet' : 'turquoise'}
    >
      <h2>Ранний BSC</h2>
      <p>Эти значения несопоставимы с BSC v2.</p>
      <p>Протокол, контроль и наблюдения вызовов в этом формате не сохранялись.</p>
      <nav class={styles.experimentHistory__tabs} aria-label="Участники раннего результата">
        <For each={props.result.people}>
          {(value, index) => (
            <button
              type="button"
              aria-pressed={selected() === index()}
              onClick={() => setSelected(index())}
            >
              {value.profile.label}
            </button>
          )}
        </For>
      </nav>
      <p>
        Записанный BSC: {person().score.bsc} · записанное время: {person().score.reasoningMinutes}{' '}
        мин. · отношение A/B: {props.result.comparison.ratio ?? 'не сохранено'}
      </p>
      <p>Сводка CLI: {person().attempt.trace.summary}</p>
      <nav class={styles.experimentHistory__tabs} aria-label="Разделы раннего результата">
        <For
          each={[
            ['answer', 'Ответ'],
            ['profile', 'Профиль'],
            ['sources', 'Источники'],
            ['comparison', 'Сравнение'],
            ['input', 'Общая задача'],
          ]}
        >
          {([id, label]) => (
            <button type="button" aria-pressed={detail() === id} onClick={() => setDetail(id!)}>
              {label}
            </button>
          )}
        </For>
      </nav>
      <PagedText text={text()} />
    </section>
  );
}
