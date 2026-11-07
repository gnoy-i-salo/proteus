import { Show } from 'solid-js';
import type { ExperimentRunModel } from '../model/experiment-run';
import { protocolText } from './experiment-report';
import { TraceReader } from './trace-reader';
import { PagedText } from './paged-text';
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
    | 'stop'
  >;
  readonly selected: number;
}) {
  return (
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
        <Show
          when={props.run.pendingProtocol()}
          fallback={
            <TraceReader
              stages={props.run.reasoningStages()}
              running={props.run.running()}
              person={props.selected}
            />
          }
        >
          {(protocol) => <PagedText text={protocolText(protocol())} resetKey="protocol" />}
        </Show>
      </div>
      <nav class={styles['experiment__tracePages']}>
        <span>{props.run.pendingProtocol() ? 'Протокол' : props.run.events().at(-1)?.message}</span>
        <Show when={props.run.pendingProtocol()}>
          <button onClick={() => props.run.approveProtocol()}>Принять протокол</button>
        </Show>
        <button
          disabled={!props.run.running()}
          onClick={() => {
            props.run.stop();
          }}
        >
          Остановить
        </button>
      </nav>
      <footer class={styles['experiment__progress']}>
        <span>PROGRESS {Math.round((props.run.progress() / props.run.total()) * 100)}%</span>
        <progress max={props.run.total()} value={props.run.progress()} />
      </footer>
    </>
  );
}
