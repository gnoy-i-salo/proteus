import { Show } from 'solid-js';
import type { ExperimentRunModel } from '../model/experiment-run';
import { TraceReader } from './trace-reader';
import { ProtocolReview } from './protocol-review';
import styles from './experiment-page.module.scss';

export function ExperimentProgressView(props: {
  readonly run: Pick<
    ExperimentRunModel,
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
}) {
  return (
    <Show
      when={props.run.pendingProtocol()}
      fallback={
        <>
          <header class={styles['experiment__progressHeading']}>
            <span>
              LIVE EXPERIMENT / {props.run.progress()} OF {props.run.total()}
            </span>
            <h2>
              Intelligence traces<span class={styles['experiment__blink']}>_</span>
            </h2>
            <p>{props.run.activity()}</p>
          </header>
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
            <span>PROGRESS {Math.round((props.run.progress() / props.run.total()) * 100)}%</span>
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
