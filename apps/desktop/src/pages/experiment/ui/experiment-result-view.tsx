import { For, Show } from 'solid-js';
import type { ExperimentRunModel } from '../model/experiment-run';
import type { TraceNavigation } from '../model/trace-navigation';
import { formatInterval, formatScore, reportText, sourceLabel } from './experiment-report';
import { ResultAnalysis, ResultComparison } from './result-analysis';
import { FailureReason } from './failure-reason';
import { TraceReader } from './trace-reader';
import { PagedText } from './paged-text';
import { ReplyReader } from './reply-reader';
import styles from './experiment-page.module.scss';

export function ExperimentResultView(props: {
  readonly run: Pick<ExperimentRunModel, 'result' | 'savedPath' | 'download' | 'failureReason'>;
  readonly traces: TraceNavigation;
  readonly selected: number;
  readonly detail: string;
  readonly onDetail: (detail: string) => void;
}) {
  const persona = () =>
    props.selected === 2 ? props.run.result()?.control : props.run.result()?.people[props.selected];
  const text = () => {
    const current = persona();
    const output = props.run.result();
    if (!current || !output) return '';
    return reportText(output, current, props.detail);
  };
  return (
    <>
      <header class={styles['experiment__resultBanner']}>
        <span>
          {props.run.result()?.status === 'complete'
            ? 'EXPERIMENT COMPLETE'
            : 'EXPERIMENT INCOMPLETE'}
        </span>
        <h2>
          {props.selected === 3
            ? 'Сравнение участников'
            : `BSC v2 · ${formatScore(persona()?.score)}`}
        </h2>
        <p>Разница A − B: {formatInterval(props.run.result()?.comparison.difference)} п.п.</p>
        <p>
          {props.run.result()?.status !== 'complete'
            ? 'Серия не завершена · итоговое сравнение недоступно'
            : props.run.result()?.protocol.repetitions === 1
              ? 'Предварительный обзор · один прогон'
              : 'Описание трёх прогонов · статистическое превосходство не установлено'}
        </p>
        <Show when={props.run.failureReason()}>
          <FailureReason
            label="Причина неполной серии"
            reason={props.run.failureReason()}
            retained="Сохранённые ответы и этапы доступны. Можно сохранить JSON или начать новый запуск."
          />
        </Show>
      </header>
      <div class={styles['experiment__resultBody']}>
        <Show
          when={props.selected === 3}
          fallback={
            <>
              <div class={styles['experiment__resultHeading']}>
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
                      props.detail === 'answer' &&
                      persona()?.trials.some((trial) => trial.attempt?.reply)
                    }
                    fallback={
                      <Show
                        when={props.detail === 'analysis' && persona()}
                        fallback={<PagedText text={text()} />}
                      >
                        {(person) => (
                          <ResultAnalysis output={props.run.result()!} person={person()} />
                        )}
                      </Show>
                    }
                  >
                    <ReplyReader person={persona()!} />
                  </Show>
                }
              >
                <TraceReader navigation={props.traces} running={false} />
              </Show>
            </>
          }
        >
          <ResultComparison output={props.run.result()!} />
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
