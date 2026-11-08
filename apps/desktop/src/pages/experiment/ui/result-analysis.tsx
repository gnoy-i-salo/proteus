import { For, Show } from 'solid-js';
import type { ExperimentPerson, ExperimentResult } from '@proteus-int/pipeline';
import { TextWorkspace } from '../../../shared/ui/text-workspace/text-workspace';
import { locateQuote, type QuoteLocation } from '../model/reply-navigation';
import { formatInterval, formatScore, reportText } from './experiment-report';
import styles from './result-analysis.module.scss';

const requiredLabels = { passed: 'выполнены', failed: 'не выполнены', unknown: 'не установлено' };
const assessmentLabels = {
  met: 'Выполнено',
  partial: 'Частично',
  unmet: 'Не выполнено',
  unknown: 'Неизвестно',
};
const quoteStatusLabels = (location: QuoteLocation | undefined): string => {
  if (location?.kind === 'found') return 'Цитата найдена в опубликованном ответе';
  if (location?.kind === 'absent') return 'Цитата не найдена в опубликованном ответе';
  if (location?.kind === 'unavailable') return 'Ответ прогона не опубликован';
  return '';
};

export function ResultAnalysis(props: {
  readonly output: ExperimentResult;
  readonly person: ExperimentPerson;
  readonly mode: () => string;
  readonly onMode: (mode: string) => void;
  readonly selected: () => number;
  readonly onSelected: (selected: number) => void;
  readonly onRevealQuote: (target: { quote: string; round: number }) => void;
}) {
  const entries = () =>
    props.person.trials.flatMap((trial) =>
      props.output.protocol.rubric.criteria.map((criterion) => ({ trial, criterion })),
    );
  const entry = () => entries()[props.selected()] ?? entries()[0];
  const assessment = (order: number) =>
    entry()?.trial.evaluations[order]?.criteria.find(
      (item) => item.criterionId === entry()?.criterion.id,
    );
  const locateAssessmentQuote = (quote: string | undefined) =>
    quote ? locateQuote(entry()?.trial.attempt ?? null, quote) : undefined;
  const details = () =>
    [
      `Прогон ${entry()?.trial.round} · ${entry()?.criterion.label}`,
      ...[0, 1].map((order) => {
        const value = assessment(order);
        return `ПОРЯДОК ${order + 1} · ${value ? assessmentLabels[value.status] : 'Не оценено'}\nОснование:\n${value?.reason || 'не получено'}\n\nЦитата ответа:\n${value?.quote || 'не представлена'}`;
      }),
    ].join('\n\n');
  return (
    <section class={styles.resultAnalysis} aria-label="Анализ результата">
      <nav class={styles.resultAnalysis__navigation} aria-label="Представление анализа">
        <For each={['summary', 'criteria', 'report']}>
          {(value) => (
            <button aria-pressed={props.mode() === value} onClick={() => props.onMode(value)}>
              {{ summary: 'Сводка', criteria: 'Критерии', report: 'Полный отчёт' }[value]}
            </button>
          )}
        </For>
      </nav>
      <Show when={props.mode() === 'summary'}>
        <div class={styles.resultAnalysis__summary}>
          <p>
            Оценено {props.person.score.evaluated} из {props.person.score.planned} прогонов
          </p>
          <p data-required={props.person.score.requiredStatus}>
            Обязательные требования: {requiredLabels[props.person.score.requiredStatus]}
          </p>
          <p>Расхождение оценок: {props.person.score.judgeDisagreement ? 'есть' : 'нет'}</p>
          <p>Разброс: {formatInterval(props.person.score.spread)}</p>
          <Show when={props.person !== props.output.control}>
            <p>Разница с контролем: {formatInterval(props.person.baselineDifference)} п.п.</p>
          </Show>
        </div>
      </Show>
      <Show when={props.mode() === 'criteria'}>
        <label>
          Критерий и прогон
          <select
            value={props.selected()}
            onChange={(event) => props.onSelected(Number(event.currentTarget.value))}
          >
            <For each={entries()}>
              {(item, index) => (
                <option value={index()}>
                  Прогон {item.trial.round} · {item.criterion.label}
                </option>
              )}
            </For>
          </select>
        </label>
        <table aria-label="Матрица критериев">
          <thead>
            <tr>
              <th scope="col">Критерий</th>
              <th scope="col">Порядок 1</th>
              <th scope="col">Порядок 2</th>
            </tr>
          </thead>
          <tbody>
            <tr>
              <th scope="row">
                <button aria-label="Развернуть критерий" onClick={() => props.onMode('details')}>
                  Основания и цитаты
                </button>
                <small>
                  Вес {entry()?.criterion.weight} ·{' '}
                  {entry()?.criterion.required ? 'обязательный' : 'необязательный'}
                </small>
              </th>
              <For each={[0, 1]}>
                {(order) => (
                  <td>
                    {assessment(order) ? assessmentLabels[assessment(order)!.status] : 'Не оценено'}
                  </td>
                )}
              </For>
            </tr>
          </tbody>
        </table>
      </Show>
      <Show when={props.mode() === 'details'}>
        <button onClick={() => props.onMode('criteria')}>← К матрице</button>
        <div class={styles.resultAnalysis__quoteActions}>
          <For each={[0, 1]}>
            {(order) => {
              const value = () => assessment(order);
              const location = () => locateAssessmentQuote(value()?.quote);
              return (
                <div class={styles.resultAnalysis__quoteAction}>
                  <span>Порядок {order + 1}</span>
                  <Show
                    when={value()?.quote}
                    fallback={<span role="status">Цитата не представлена</span>}
                  >
                    <button
                      type="button"
                      onClick={() =>
                        props.onRevealQuote({
                          quote: value()!.quote,
                          round: entry()!.trial.round,
                        })
                      }
                    >
                      Показать в ответе
                    </button>
                    <span role="status">{quoteStatusLabels(location())}</span>
                  </Show>
                </div>
              );
            }}
          </For>
        </div>
        <TextWorkspace label="Основания и точные цитаты" value={details()} resetKey={details()} />
      </Show>
      <Show when={props.mode() === 'report'}>
        <TextWorkspace
          label="Полный отчёт"
          value={reportText(props.output, props.person, 'analysis')}
        />
      </Show>
    </section>
  );
}

export function ResultComparison(props: { readonly output: ExperimentResult }) {
  return (
    <section class={styles.resultAnalysis} aria-label="Сравнение результатов">
      <table aria-label="Сравнение участников">
        <thead>
          <tr>
            <th scope="col">Участник</th>
            <th scope="col">BSC v2</th>
            <th scope="col">С контролем</th>
            <th scope="col">Обязательные</th>
          </tr>
        </thead>
        <tbody>
          <For each={[...props.output.people, props.output.control]}>
            {(person, index) => (
              <tr>
                <th scope="row">{index() === 2 ? 'Контроль' : index() === 0 ? 'A' : 'B'}</th>
                <td>
                  {formatScore(person.score)}
                  <small>
                    {person.score.evaluated}/{person.score.planned} прогонов
                  </small>
                </td>
                <td>
                  {index() === 2
                    ? 'Базовый уровень'
                    : `${formatInterval(person.baselineDifference)} п.п.`}
                </td>
                <td data-required={person.score.requiredStatus}>
                  {requiredLabels[person.score.requiredStatus]}
                </td>
              </tr>
            )}
          </For>
        </tbody>
      </table>
      <p>Это не доказательство статистического превосходства.</p>
    </section>
  );
}
