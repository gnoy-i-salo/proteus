import { createSignal, Show } from 'solid-js';
import { SettingsDialog } from '../../../widgets/settings-dialog';
import { SourceSelection } from '../../../features/source-selection';
import { createExperimentInput } from '../model/experiment-input';
import { createExperimentSources } from '../model/experiment-sources';
import { createExperimentSettings } from '../model/experiment-settings';
import { createExperimentPresets } from '../model/experiment-presets';
import { createExperimentRun } from '../model/experiment-run';
import { createSavedExperimentHistory, reuseHistoryConditions } from '../model/experiment-history';
import { createTraceNavigation } from '../model/trace-navigation';
import { ExperimentEntry } from './experiment-entry';
import { ExperimentWorkspace } from './experiment-workspace';
import { ExperimentHistory } from './experiment-history';
import styles from './experiment-page.module.scss';

export function ExperimentPage() {
  let fileInput: HTMLInputElement | undefined;
  const [selected, setSelected] = createSignal(0);
  const [settingsOpen, setSettingsOpen] = createSignal(false);
  const [sourcePerson, setSourcePerson] = createSignal<number>();
  const [error, setError] = createSignal('');
  const [detail, setDetail] = createSignal('analysis');
  const [historyOpen, setHistoryOpen] = createSignal(false);
  const history = createSavedExperimentHistory();
  const showHistory = () => {
    setHistoryOpen(true);
    void history
      .refresh()
      .catch(() => setError('Не удалось прочитать историю. Повторите попытку.'));
  };
  const hideHistory = () => {
    history.cancel();
    setHistoryOpen(false);
  };
  const {
    sourceGrants,
    sourceImports,
    clearSourceGrants,
    rememberImport,
    clearProvider,
    changeGrants,
  } = createExperimentSources(undefined, setError);
  const { settings, sourceModelsAvailable, persist } = createExperimentSettings({
    clearSourceGrants,
    reportError: setError,
  });
  const input = createExperimentInput();
  const run = createExperimentRun({
    currentInput: input.currentInput,
    settings,
    repetitions: input.repetitions,
    sourceGrants,
    sourceModelsAvailable,
    reportError: setError,
    onStarted: () => {
      traces.reset();
      setSelected(0);
    },
    onCompleted: () => setDetail(traces.manual() ? 'traces' : 'analysis'),
  });
  const traces = createTraceNavigation({
    stages: run.reasoningStages,
    running: run.running,
    activeStageId: run.activeStageId,
    failedStageId: run.failedStageId,
  });
  const { pendingPreset, openPreset, savePreset, readBrowserFile, applyPending, dismissPending } =
    createExperimentPresets({
      running: run.running,
      currentInput: input.currentInput,
      applyInput: input.applyInput,
      clearSourceGrants,
      reportError: setError,
      onApplied: () => {
        run.setScreen('start');
        setSelected(0);
      },
      openBrowserFile: () => fileInput?.click(),
    });
  return (
    <main class={styles['experiment']} data-palette={selected() === 0 ? 'turquoise' : 'violet'}>
      <Show
        when={!historyOpen()}
        fallback={
          <ExperimentHistory
            history={history}
            onError={setError}
            onClose={hideHistory}
            onReuse={(value) => {
              reuseHistoryConditions(
                value.result.input,
                value.sourceVersion === 1 ? 1 : value.result.protocol.repetitions,
                {
                  running: run.running,
                  clearSourceGrants,
                  applyInput: input.applyInput,
                  setRepetitions: input.setRepetitions,
                  onApplied: () => {
                    run.setScreen('start');
                    hideHistory();
                  },
                },
              );
            }}
          />
        }
      >
        <Show
          when={run.screen() === 'start'}
          fallback={
            <ExperimentWorkspace
              run={run}
              traces={traces}
              names={input.names()}
              selected={selected()}
              detail={detail()}
              onSelect={setSelected}
              onDetail={setDetail}
              onNewRun={() => {
                run.setScreen('start');
                setError('');
              }}
              onShowSettings={() => setSettingsOpen(true)}
              onShowHistory={showHistory}
            />
          }
        >
          <ExperimentEntry
            input={input}
            available={run.available()}
            sourceCounts={sourceGrants().map((grants) => grants.length)}
            onStart={() =>
              void run
                .start()
                .catch(() =>
                  setError(
                    'Не удалось начать эксперимент. Проверьте настройки и повторите попытку.',
                  ),
                )
            }
            onOpenPreset={() =>
              void openPreset().catch(() =>
                setError('Не удалось открыть пресет. Повторите попытку.'),
              )
            }
            onSavePreset={() =>
              void savePreset().catch(() =>
                setError('Не удалось сохранить пресет. Повторите попытку.'),
              )
            }
            onShowSettings={() => setSettingsOpen(true)}
            onShowHistory={showHistory}
            onShowSources={setSourcePerson}
          />
        </Show>
      </Show>
      <input
        ref={(element) => {
          fileInput = element;
        }}
        type="file"
        hidden
        accept=".proteus"
        aria-label="Файл пресета"
        onChange={(event) => {
          const file = event.currentTarget.files?.[0];
          event.currentTarget.value = '';
          void readBrowserFile(file).catch(() =>
            setError('Не удалось прочитать пресет. Повторите попытку.'),
          );
        }}
      />
      <Show when={pendingPreset()}>
        <div class={styles['experiment__pending']} role="status">
          <span>
            {run.running() ? 'Пресет ожидает завершения эксперимента.' : 'Пресет готов к открытию.'}
          </span>
          <button
            disabled={run.running()}
            onClick={() => {
              applyPending();
            }}
          >
            Открыть пресет
          </button>
          <button aria-label="Отложить пресет" onClick={dismissPending}>
            ×
          </button>
        </div>
      </Show>
      <Show when={error() && error() !== run.failureNotification()}>
        <div class={styles['experiment__error']} role="alert">
          <p>{error()}</p>
          <Show
            when={
              run.readinessError() ||
              (!sourceModelsAvailable() && sourceGrants().some((grants) => grants.length > 0))
            }
          >
            <button onClick={() => setSettingsOpen(true)}>Открыть настройки моделей</button>
          </Show>
          <button aria-label="Закрыть сообщение" onClick={() => setError('')}>
            ×
          </button>
        </div>
      </Show>
      <Show when={settingsOpen()}>
        <div
          class={styles['experiment__modal']}
          onKeyDown={(event) => {
            if (event.key === 'Escape') setSettingsOpen(false);
          }}
        >
          <SettingsDialog
            settings={settings()}
            hasSources={sourceGrants().some((grants) => grants.length > 0)}
            available={run.available()}
            onChange={persist}
            onClose={() => setSettingsOpen(false)}
            onImported={rememberImport}
            onDataCleared={clearProvider}
          />
        </div>
      </Show>
      <Show when={sourcePerson() !== undefined}>
        <div class={styles['experiment__modal']}>
          <SourceSelection
            letter={sourcePerson() === 0 ? 'A' : 'B'}
            model={settings().components.stalker === 'codex' ? 'Codex' : 'Kimi'}
            modelProcessingAvailable={sourceModelsAvailable()}
            grants={sourceGrants()[sourcePerson() ?? 0] ?? []}
            imports={sourceImports()}
            onImported={rememberImport}
            onChange={(grants) => changeGrants(sourcePerson() === 0 ? 0 : 1, grants)}
            onClose={() => setSourcePerson(undefined)}
            onShowSettings={() => {
              setSourcePerson(undefined);
              setSettingsOpen(true);
            }}
          />
        </div>
      </Show>
    </main>
  );
}
