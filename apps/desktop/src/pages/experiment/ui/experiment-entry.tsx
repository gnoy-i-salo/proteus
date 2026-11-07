import { For } from 'solid-js';
import type { CliStatus } from '../../../shared/lib/cli';
import type { ExperimentInputModel } from '../model/experiment-input';
import { PagedField } from './paged-field';
import { PublicLinks } from './public-links';
import styles from './experiment-page.module.scss';

export function ExperimentEntry(props: {
  readonly input: ExperimentInputModel;
  readonly available: readonly CliStatus[];
  readonly sourceCounts: readonly number[];
  readonly onStart: () => void;
  readonly onOpenPreset: () => void;
  readonly onSavePreset: () => void;
  readonly onShowSettings: () => void;
  readonly onShowSources: (person: number) => void;
}) {
  return (
    <form
      class={styles['experiment__start']}
      onSubmit={(event) => {
        event.preventDefault();
        void props.onStart();
      }}
    >
      <header class={styles['experiment__hero']}>
        <div class={styles['experiment__topline']}>
          <div class={styles['experiment__preset-actions']}>
            <button type="button" onClick={() => void props.onOpenPreset()}>
              Импорт .proteus
            </button>
            <button type="button" onClick={() => void props.onSavePreset()}>
              Экспорт .proteus
            </button>
          </div>
          <button
            type="button"
            aria-label="Настройки PROTEUS"
            onClick={() => props.onShowSettings()}
          >
            ⚙ CONFIG
          </button>
        </div>
        <img src="/proteus-logo.svg" alt="Трёхглавый символ PROTEUS" />
        <div>
          <h1>PROTEUS</h1>
          <p>ONE TASK. MANY MINDS.</p>
        </div>
      </header>
      <div class={styles['experiment__duel']}>
        <For each={['A', 'B']}>
          {(letter, index) => (
            <section class={styles['experiment__input-card']} data-tone={letter.toLowerCase()}>
              <header>
                <span>
                  PLAYER {index() + 1 < 10 ? '0' : ''}
                  {index() + 1}
                </span>
                <strong>PERSON {letter}</strong>
                <button
                  type="button"
                  class={styles['experiment__source-button']}
                  aria-label={`Источники персоны ${letter}`}
                  onClick={() => props.onShowSources(index())}
                >
                  Источники {props.sourceCounts[index()] ?? 0}
                </button>
              </header>
              <div class={styles['experiment__identity']}>
                <label>
                  Имя
                  <input
                    aria-label={`Имя персоны ${letter}`}
                    maxLength={80}
                    value={props.input.names()[index()]}
                    onInput={(event) =>
                      props.input.setNames(
                        props.input
                          .names()
                          .map((name, i) => (i === index() ? event.currentTarget.value : name)),
                      )
                    }
                  />
                </label>
                <PublicLinks
                  letter={letter}
                  revision={props.input.revision()}
                  urls={props.input.urls()[index()] ?? ['']}
                  onChange={(links) =>
                    props.input.setUrls(
                      props.input.urls().map((value, i) => (i === index() ? links : value)),
                    )
                  }
                />
              </div>
              <PagedField
                label={`Контекст ${letter}`}
                revision={props.input.revision()}
                pages={props.input.contexts()[index()] ?? ['']}
                onChange={(pages) =>
                  props.input.setContexts(
                    props.input.contexts().map((value, i) => (i === index() ? pages : value)),
                  )
                }
              />
            </section>
          )}
        </For>
        <span class={styles['experiment__versus']}>VS</span>
      </div>
      <div class={styles['experiment__task']}>
        <span>
          MISSION
          <br />
          <small>Одна задача для обоих</small>
        </span>
        <PagedField
          compact
          label="Общая задача"
          pages={props.input.task()}
          onChange={(pages) => props.input.setTask(pages)}
          revision={props.input.revision()}
        />
      </div>
      <footer class={styles['experiment__launch']}>
        <span>
          {props.available.length
            ? props.available
                .map((cli) => `${cli.provider.toUpperCase()} ${cli.available ? '●' : '○'}`)
                .join(' / ')
            : 'DESKTOP CLI CONNECTION'}
        </span>
        <button type="submit" aria-label="Начать эксперимент">
          START ▶
        </button>
        <select
          aria-label="Режим эксперимента"
          value={props.input.repetitions()}
          onChange={(event) =>
            props.input.setRepetitions(event.currentTarget.value === '3' ? 3 : 1)
          }
        >
          <option value="1">1 прогон · обзор</option>
          <option value="3">3 прогона · серия</option>
        </select>
      </footer>
    </form>
  );
}
