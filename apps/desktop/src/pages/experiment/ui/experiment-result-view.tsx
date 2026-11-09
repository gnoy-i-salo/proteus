import { createSignal, For, Show } from 'solid-js';
import { PersonaMemoryView } from '../../../features/persona-memory';
import type { ExperimentRunModel } from '../model/experiment-run';
import { createReplyNavigation } from '../model/reply-navigation';
import type { TraceNavigation } from '../model/trace-navigation';
import { formatInterval, formatScore, reportText, sourceLabel } from './experiment-report';
import { ResultAnalysis, ResultComparison } from './result-analysis';
import { FailureReason } from './failure-reason';
import { TraceReader } from './trace-reader';
import { PagedText } from './paged-text';
import { ReplyReader } from './reply-reader';
import styles from './experiment-page.module.scss';

export function ExperimentResultView(props: {
  readonly run: Pick<ExperimentRunModel, 'result' | 'failureReason'> &
    Partial<Pick<ExperimentRunModel, 'savedPath' | 'download'>>;
  readonly readOnly?: boolean;
  readonly traces: TraceNavigation;
  readonly selected: number;
  readonly detail: string;
  readonly onDetail: (detail: string) => void;
}) {
  const persona = () =>
    props.selected === 2 ? props.run.result()?.control : props.run.result()?.people[props.selected];
  const replies = createReplyNavigation({
    person: persona,
    personIndex: () => (props.selected === 0 || props.selected === 1 ? props.selected : 2),
    stages: () => props.run.result()?.traces ?? [],
  });
  const [analysisMode, setAnalysisMode] = createSignal('summary');
  const [analysisSelected, setAnalysisSelected] = createSignal(0);
  const [returnTab, setReturnTab] = createSignal<string | undefined>(undefined);
  const [callReturnPerson, setCallReturnPerson] = createSignal<string | undefined>(undefined);
  const text = () => {
    const current = persona();
    const output = props.run.result();
    if (!current || !output) return '';
    return reportText(output, current, props.detail);
  };
  const rememberTab = (next: string) => {
    if (props.detail !== next) setReturnTab(props.detail);
    props.onDetail(next);
  };
  const revealQuote = (target: { quote: string; round: number }) => {
    replies.revealQuote(target);
    rememberTab('answer');
  };
  const openCall = (stageId: string) => {
    props.traces.select(stageId);
    setCallReturnPerson(persona()?.profile.id);
    props.onDetail('traces');
  };
  const returnToPriorTab = () => {
    const target = returnTab();
    setReturnTab(undefined);
    props.onDetail(target ?? 'analysis');
  };
  return (
    <>
      <header class={styles['experiment__resultBanner']}>
        <span>
          {props.readOnly
            ? 'СОХРАНЁННЫЙ РЕЗУЛЬТАТ'
            : props.run.result()?.status === 'complete'
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
            retained={
              props.readOnly
                ? 'Сохранённые ответы и этапы доступны для чтения.'
                : 'Сохранённые ответы и этапы доступны. Можно сохранить JSON или начать новый запуск.'
            }
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
                    ['archetype', 'Профиль'],
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
                <Show
                  when={
                    props.detail === 'traces' &&
                    callReturnPerson() !== undefined &&
                    callReturnPerson() === persona()?.profile.id
                  }
                >
                  <button
                    type="button"
                    onClick={() => {
                      setCallReturnPerson(undefined);
                      props.onDetail('answer');
                    }}
                  >
                    ← Вернуться к ответу
                  </button>
                </Show>
              </nav>
              <Show
                when={props.detail === 'traces'}
                fallback={
                  <Show
                    when={props.detail === 'answer' && persona()}
                    fallback={
                      <Show
                        when={props.detail === 'analysis' && persona()}
                        fallback={
                          <Show
                            when={props.detail === 'archetype' && persona()}
                            fallback={<PagedText text={text()} />}
                          >
                            {(person) => (
                              <PersonaMemoryView
                                profile={person().profile}
                                baseProfile={person().baseProfile}
                              />
                            )}
                          </Show>
                        }
                      >
                        {(person) => (
                          <ResultAnalysis
                            output={props.run.result()!}
                            person={person()}
                            mode={analysisMode}
                            onMode={setAnalysisMode}
                            selected={analysisSelected}
                            onSelected={setAnalysisSelected}
                            onRevealQuote={revealQuote}
                          />
                        )}
                      </Show>
                    }
                  >
                    <ReplyReader
                      navigation={replies}
                      onOpenCall={openCall}
                      onReturn={returnToPriorTab}
                      canReturn={returnTab() !== undefined}
                    />
                  </Show>
                }
              >
                <TraceReader
                  navigation={props.traces}
                  running={false}
                  result={props.run.result() ?? undefined}
                />
              </Show>
            </>
          }
        >
          <ResultComparison output={props.run.result()!} />
        </Show>
      </div>
      <footer class={styles['experiment__footer']}>
        <span title={props.run.savedPath?.()}>
          {props.readOnly
            ? 'Просмотр истории · исходный файл не изменён'
            : props.run.savedPath?.()
              ? '✓ Сохранено локально'
              : 'Результат готов'}
        </span>
        <Show when={!props.readOnly && props.run.download}>
          <button onClick={() => props.run.download?.()}>↓ JSON</button>
        </Show>
      </footer>
    </>
  );
}
