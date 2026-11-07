import { For, Show } from 'solid-js';
import type { ExperimentRunModel } from '../model/experiment-run';
import { formatInterval, formatScore, reportText, sourceLabel } from './experiment-report';
import { TraceReader } from './trace-reader';
import { PagedText } from './paged-text';
import { ReplyReader } from './reply-reader';
import styles from './experiment-page.module.scss';

export function ExperimentResultView(props: {
  readonly run: Pick<ExperimentRunModel, 'result' | 'savedPath' | 'reasoningStages' | 'download'>;
  readonly selected: number;
  readonly detail: string;
  readonly onDetail: (detail: string) => void;
}) {
  const persona = () => props.run.result()?.people[props.selected];
  const text = () => {
    const current = persona();
    const output = props.run.result();
    if (!current || !output) return '';
    return reportText(output, current, props.detail);
  };
  return (
    <>
      <header class={styles['experiment__result-banner']}>
        <span>
          {props.run.result()?.status === 'complete'
            ? 'EXPERIMENT COMPLETE'
            : 'EXPERIMENT INCOMPLETE'}
        </span>
        <h2>BSC v2 · {formatScore(persona()?.score)}</h2>
        <p>Разница A − B: {formatInterval(props.run.result()?.comparison.difference)} п.п.</p>
        <p>
          {props.run.result()?.status !== 'complete'
            ? 'Серия не завершена · итоговое сравнение недоступно'
            : props.run.result()?.protocol.repetitions === 1
              ? 'Предварительный обзор · один прогон'
              : 'Описание трёх прогонов · статистическое превосходство не установлено'}
        </p>
      </header>
      <div class={styles['experiment__result-body']}>
        <div class={styles['experiment__result-heading']}>
          <h3>{persona()?.profile.label}</h3>
          <span>{sourceLabel(persona())}</span>
        </div>
        <nav class={styles['experiment__tabs']} aria-label="Разделы анализа">
          <For
            each={[
              ['analysis', 'Анализ'],
              ['answer', 'Ответ'],
              ['archetype', 'Архетип'],
              ['sources', 'Источники'],
              ['traces', 'Intelligence traces'],
            ]}
          >
            {(item) => (
              <button
                aria-pressed={props.detail === item[0]}
                onClick={() => props.onDetail(item[0] ?? 'analysis')}
              >
                {item[1]}
              </button>
            )}
          </For>
        </nav>
        <Show
          when={props.detail === 'traces'}
          fallback={
            <Show
              when={
                props.detail === 'answer' && persona()?.trials.some((trial) => trial.attempt?.reply)
              }
              fallback={<PagedText text={text()} />}
            >
              <ReplyReader person={persona()!} />
            </Show>
          }
        >
          <TraceReader
            stages={props.run.reasoningStages()}
            running={false}
            person={props.selected}
          />
        </Show>
      </div>
      <footer class={styles['experiment__footer']}>
        <span title={props.run.savedPath()}>
          {props.run.savedPath() ? '✓ Сохранено локально' : 'Результат готов'}
        </span>
        <button onClick={() => props.run.download()}>↓ JSON</button>
      </footer>
    </>
  );
}
