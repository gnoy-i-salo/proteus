import { Show } from 'solid-js';
import type { ExperimentRunModel } from '../model/experiment-run';
import { classifyExecutionStage } from '../model/execution-stage';
import type { RunStatus } from '../model/execution-state';
import { FailureReason } from './failure-reason';
import styles from './execution-status.module.scss';
const statusLabels: Record<RunStatus, string> = {
  idle: 'Ожидает запуска',
  preparing: 'Подготовка',
  running: 'Выполняется',
  waitingForProtocol: 'Ожидает подтверждения',
  stopping: 'Останавливается',
  completed: 'Завершён',
  incomplete: 'Серия не завершена',
  failed: 'Ошибка этапа',
  cancelled: 'Отменён',
};
export function ExecutionStatus(props: {
  readonly run: Pick<
    ExperimentRunModel,
    'runStatus' | 'displayStage' | 'elapsedMs' | 'lastCliEventAgeMs' | 'failureReason'
  >;
  readonly names: readonly string[];
}) {
  const active = () => ['preparing', 'running'].includes(props.run.runStatus());
  const stage = () => props.run.displayStage();
  const identity = () => {
    const current = stage();
    return current ? classifyExecutionStage(current) : undefined;
  };
  const participant = () => {
    switch (identity()?.participant) {
      case 'a':
        return `A · ${props.names[0] ?? 'Персона A'}`;
      case 'b':
        return `B · ${props.names[1] ?? 'Персона B'}`;
      case 'control':
        return 'Контроль';
      case 'system':
        return 'Система';
      default:
        return 'Участник не определён';
    }
  };
  return (
    <section class={styles['executionStatus']} aria-label="Состояние запуска">
      <strong>{statusLabels[props.run.runStatus()]}</strong>
      <Show when={active()}>
        <h2>
          Intelligence traces
          <span data-active="true" class={styles['executionStatus__blink']}>
            _
          </span>
        </h2>
      </Show>
      <p>
        <Show when={stage()} fallback="Подготовка запуска">
          {identity()?.componentLabel} · {participant()} · {stage()?.label}
          <Show when={identity()?.round}>
            {' · Прогон '}
            {identity()?.round}
          </Show>
        </Show>
      </p>
      <p>
        С начала запуска:{' '}
        <span data-testid="host-elapsed">{Math.floor(props.run.elapsedMs() / 1000)} с</span>
        {' · '}
        {props.run.lastCliEventAgeMs() === null
          ? 'Событий CLI ещё не было'
          : `Последнее событие CLI: ${Math.floor(props.run.lastCliEventAgeMs()! / 1000)} с назад`}
      </p>
      <Show when={active()}>
        <small>Часы PROTEUS не показывают активность или рассуждение провайдера.</small>
      </Show>
      <Show when={props.run.failureReason()}>
        <FailureReason
          label="Причина остановки"
          reason={props.run.failureReason()}
          retained="Выполнение остановлено в PROTEUS. Сохранённые этапы доступны ниже. Для нового запуска вернитесь к вводу."
        />
      </Show>
    </section>
  );
}
