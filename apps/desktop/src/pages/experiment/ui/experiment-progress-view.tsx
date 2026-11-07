import { Show } from 'solid-js';
import type { ExperimentRunModel } from '../model/experiment-run';
import { TraceReader } from './trace-reader';
import { ProtocolReview } from './protocol-review';
import { ExecutionStatus } from './execution-status';
import styles from './experiment-page.module.scss';

export function ExperimentProgressView(props: {
  readonly run: Pick<
    ExperimentRunModel,
    | 'runStatus'
    | 'displayStage'
    | 'elapsedMs'
    | 'lastCliEventAgeMs'
    | 'failureReason'
    | 'activity'
    | 'progress'
    | 'total'
    | 'pendingProtocol'
    | 'reasoningStages'
    | 'running'
    | 'events'
    | 'approveProtocol'
    | 'returnToInput'
    | 'stop'
  >;
  readonly selected: number;
  readonly names: readonly string[];
}) {
  return (
    <Show
      when={props.run.pendingProtocol()}
      fallback={
        <>
          <ExecutionStatus run={props.run} names={props.names} />
          <div class={styles['experiment__traces']}>
            <TraceReader
              stages={props.run.reasoningStages()}
              running={props.run.running()}
              person={props.selected}
            />
          </div>
          <nav class={styles['experiment__tracePages']}>
            <span>{props.run.events().at(-1)?.message}</span>
            <button disabled={!props.run.running()} onClick={() => props.run.stop()}>
              Остановить
            </button>
          </nav>
          <footer class={styles['experiment__progress']}>
            <span>
              Завершено этапов: {props.run.progress()} / {props.run.total()}
            </span>
            <progress max={props.run.total()} value={props.run.progress()} />
          </footer>
        </>
      }
    >
      {(protocol) => (
        <ProtocolReview
          protocol={protocol()}
          onApprove={() => props.run.approveProtocol()}
          onReturn={() => props.run.returnToInput()}
        />
      )}
    </Show>
  );
}
