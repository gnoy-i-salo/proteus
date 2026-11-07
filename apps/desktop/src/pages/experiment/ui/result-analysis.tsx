import { createSignal, For, Show } from 'solid-js';
import type { ExperimentPerson, ExperimentResult } from '@proteus-int/pipeline';
import { TextWorkspace } from '../../../shared/ui/text-workspace/text-workspace';
import { formatInterval, formatScore, reportText } from './experiment-report';
import styles from './result-analysis.module.scss';

const requiredLabels = { passed: 'выполнены', failed: 'не выполнены', unknown: 'не установлено' };
const assessmentLabels = {
  met: 'Выполнено',
  partial: 'Частично',
  unmet: 'Не выполнено',
  unknown: 'Неизвестно',
};

export function ResultAnalysis(props: {
  readonly output: ExperimentResult;
  readonly person: ExperimentPerson;
}) {
  const [mode, setMode] = createSignal('summary');
  const [selected, setSelected] = createSignal(0);
  const entries = () =>
    props.person.trials.flatMap((trial) =>
      props.output.protocol.rubric.criteria.map((criterion) => ({ trial, criterion })),
    );
  const entry = () => entries()[selected()] ?? entries()[0];
  const assessment = (order: number) =>
    entry()?.trial.evaluations[order]?.criteria.find(
      (item) => item.criterionId === entry()?.criterion.id,
    );
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
            <button aria-pressed={mode() === value} onClick={() => setMode(value)}>
              {{ summary: 'Сводка', criteria: 'Критерии', report: 'Полный отчёт' }[value]}
            </button>
          )}
        </For>
      </nav>
      <Show when={mode() === 'summary'}>
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
      <Show when={mode() === 'criteria'}>
        <label>
          Критерий и прогон
          <select
            value={selected()}
            onChange={(event) => setSelected(Number(event.currentTarget.value))}
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
                <button aria-label="Развернуть критерий" onClick={() => setMode('details')}>
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
      <Show when={mode() === 'details'}>
        <button onClick={() => setMode('criteria')}>← К матрице</button>
        <TextWorkspace label="Основания и точные цитаты" value={details()} resetKey={details()} />
      </Show>
      <Show when={mode() === 'report'}>
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
