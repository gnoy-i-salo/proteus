import { For, Show } from 'solid-js';
import type { ExperimentRunModel } from '../model/experiment-run';
import type { TraceNavigation } from '../model/trace-navigation';
import { ExperimentResultView } from './experiment-result-view';
import { ExperimentProgressView } from './experiment-progress-view';
import styles from './experiment-page.module.scss';

export function ExperimentWorkspace(props: {
  readonly run: ExperimentRunModel;
  readonly traces: TraceNavigation;
  readonly names: readonly string[];
  readonly selected: number;
  readonly detail: string;
  readonly onSelect: (person: number) => void;
  readonly onDetail: (detail: string) => void;
  readonly onNewRun: () => void;
  readonly onShowSettings: () => void;
  readonly onShowHistory: () => void;
}) {
  return (
    <div class={styles['experiment__workspace']}>
      <aside class={styles['experiment__sidebar']}>
        <div class={styles['experiment__brand']}>
          <img src="/proteus-logo.svg" alt="" />
          <strong>PROTEUS</strong>
        </div>
        <span class={styles['experiment__eyebrow']}>SELECT PERSON</span>
        <For each={props.names}>
          {(name, index) => (
            <button
              class={styles['experiment__person']}
              data-active={props.selected === index()}
              data-tone={index() === 0 ? 'a' : 'b'}
              onClick={() => props.onSelect(index())}
            >
              <span class={styles['experiment__avatar']} aria-hidden="true">
                <img src={index() === 0 ? '/person-a.svg' : '/person-b.svg'} alt="" />
              </span>
              <span>
                {name}
                <small>PERSON {index() === 0 ? 'A' : 'B'}</small>
              </span>
            </button>
          )}
        </For>
        <Show when={props.run.screen() !== 'progress'}>
          <For each={['Контроль', 'Сравнение']}>
            {(label, index) => (
              <button
                class={styles.experiment__person}
                aria-pressed={props.selected === index() + 2}
                data-active={props.selected === index() + 2}
                onClick={() => props.onSelect(index() + 2)}
              >
                {label}
              </button>
            )}
          </For>
        </Show>
        <div class={styles['experiment__sidebarBottom']}>
          <button disabled={props.run.running()} onClick={() => props.onShowHistory()}>
            История запусков
          </button>
          <small>
            ONE TASK.
            <br />
            MANY MINDS.
          </small>
          <button
            disabled={props.run.running()}
            onClick={() => {
              props.onNewRun();
            }}
          >
            ← Новый запуск
          </button>
          <button
            aria-label="Настройки PROTEUS"
            disabled={props.run.running()}
            onClick={() => props.onShowSettings()}
          >
            ⚙ Настройки
          </button>
        </div>
      </aside>
      <section class={styles['experiment__main']}>
        <Show
          when={props.run.screen() === 'progress'}
          fallback={
            <ExperimentResultView
              run={props.run}
              traces={props.traces}
              selected={props.selected}
              detail={props.detail}
              onDetail={(value) => props.onDetail(value)}
            />
          }
        >
          <ExperimentProgressView
            run={props.run}
            traces={props.traces}
            selected={props.selected}
            names={props.names}
          />
        </Show>
      </section>
    </div>
  );
}
