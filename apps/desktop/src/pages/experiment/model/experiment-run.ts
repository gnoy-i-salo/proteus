import { createSignal, onCleanup, onMount, untrack } from 'solid-js';
import type { ExperimentInput, ExperimentProtocol, SourceGrant } from '@proteus-int/contracts';
import type { ModelSettings } from '@proteus-int/intelligence';
import { runExperiment, validateExperiment, exportExperiment } from '@proteus-int/pipeline';
import type { ExperimentResult, ProgressEvent, StageTrace } from '@proteus-int/pipeline';
import {
  listenConnectionErrors,
  nativeSourcePort,
  resumeSourceGrant,
} from '../../../shared/lib/connections';
import { cliProviders, cliStatus, saveExperiment } from '../../../shared/lib/cli';
import type { CliStatus } from '../../../shared/lib/cli';
import { downloadFile } from '../../../shared/lib/download-file';
import { isDesktop } from '../../../shared/lib/window-controls';
import { mergeLiveReasoning } from './live-trace';
import { createExecutionState } from './execution-state';

export interface ExperimentRunPort {
  readonly cliStatus: typeof cliStatus;
  readonly isDesktop: typeof isDesktop;
  readonly listenConnectionErrors: typeof listenConnectionErrors;
  readonly validateExperiment: typeof validateExperiment;
  readonly resumeSourceGrant: typeof resumeSourceGrant;
  readonly runExperiment: typeof runExperiment;
  readonly cliProviders: typeof cliProviders;
  readonly nativeSourcePort: typeof nativeSourcePort;
  readonly saveExperiment: typeof saveExperiment;
  readonly downloadFile: typeof downloadFile;
}
export function createExperimentRun(
  options: {
    readonly currentInput: () => ExperimentInput;
    readonly settings: () => ModelSettings;
    readonly repetitions: () => 1 | 3;
    readonly sourceGrants: () => readonly [readonly SourceGrant[], readonly SourceGrant[]];
    readonly sourceModelsAvailable: () => boolean;
    readonly reportError: (message: string) => void;
    readonly onStarted: () => void;
    readonly onCompleted: () => void;
  },
  port: ExperimentRunPort = {
    cliStatus,
    isDesktop,
    listenConnectionErrors,
    validateExperiment,
    resumeSourceGrant,
    runExperiment,
    cliProviders,
    nativeSourcePort,
    saveExperiment,
    downloadFile,
  },
) {
  const executionState = createExecutionState();
  const [screen, setScreen] = createSignal<'start' | 'progress' | 'result'>('start');
  const [available, setAvailable] = createSignal<CliStatus[]>([]);
  const [pendingProtocol, setPendingProtocol] = createSignal<ExperimentProtocol>();
  let acceptProtocol: (() => void) | undefined;
  const [events, setEvents] = createSignal<ProgressEvent[]>([]);
  const [result, setResult] = createSignal<ExperimentResult>();
  const [running, setRunning] = createSignal(false);
  const [savedPath, setSavedPath] = createSignal('');
  const [failureNotification, setFailureNotification] = createSignal('');
  const [activity, setActivity] = createSignal('Ожидаем запуск');
  const [reasoningStages, setReasoningStages] = createSignal<StageTrace[]>([]);
  const recordTrace = (next: StageTrace) =>
    setReasoningStages((items) => {
      const index = items.findIndex((item) => item.id === next.id);
      return index < 0 ? [...items, next] : items.map((item, i) => (i === index ? next : item));
    });
  let controller: AbortController | undefined;
  let returningToInput = false;
  let disposed = false;
  onMount(() => {
    void port
      .cliStatus()
      .then(setAvailable)
      .catch(() => options.reportError('Не удалось проверить установленные CLI.'));
    if (port.isDesktop()) {
      let disposed = false;
      let stop: (() => void) | undefined;
      void port
        .listenConnectionErrors(() =>
          options.reportError(
            'Подключение не завершено. Откройте настройки подключений и попробуйте снова.',
          ),
        )
        .then((unlisten) => {
          if (disposed) unlisten();
          else stop = unlisten;
        })
        .catch(() => options.reportError('Не удалось включить уведомления о подключениях.'));
      onCleanup(() => {
        disposed = true;
        stop?.();
      });
    }
  });
  onCleanup(() => {
    disposed = true;
    controller?.abort();
  });
  const readinessError = () => {
    if (!available().length) return '';
    const missing = [...new Set(Object.values(options.settings().components))].filter(
      (provider) => !available().some((cli) => cli.provider === provider && cli.available),
    );
    return missing.length
      ? `${missing.map((id) => (id === 'codex' ? 'Codex' : 'Kimi')).join(', ')} CLI не найден. Установите выбранный CLI или смените провайдера в настройках. Вход выполняется в самом CLI.`
      : '';
  };
  const start = async () => {
    if (running() || disposed) return;
    options.reportError('');
    setFailureNotification('');
    setSavedPath('');
    const notReady = readinessError();
    if (notReady) {
      options.reportError(notReady);
      return;
    }
    if (
      options.sourceGrants().some((grants) => grants.length > 0) &&
      !options.sourceModelsAvailable()
    ) {
      options.reportError(
        'Для подключённых источников выберите Codex для всех компонентов в настройках. Поддержка Kimi для этих материалов пока недоступна.',
      );
      return;
    }
    const input = options.currentInput();
    try {
      port.validateExperiment(input, [
        options.sourceGrants()[0].length > 0,
        options.sourceGrants()[1].length > 0,
      ]);
    } catch (failure) {
      options.reportError(String(failure));
      return;
    }
    returningToInput = false;
    executionState.begin();
    setActivity('Подготовка запуска');
    const runController = new AbortController();
    controller = runController;
    setRunning(true);
    setEvents([]);
    setReasoningStages([]);
    setPendingProtocol(undefined);
    setResult(undefined);
    setScreen('progress');
    try {
      options.onStarted();
      for (const grant of options.sourceGrants().flat()) {
        runController.signal.throwIfAborted();
        await port.resumeSourceGrant(grant.id);
      }
      runController.signal.throwIfAborted();
      executionState.setRunStatus('running');
      const output = await port.runExperiment(
        input,
        options.settings(),
        port.cliProviders((event) =>
          untrack(() => {
            if (disposed || runController.signal.aborted) return;
            const received = executionState.receive(event.id);
            if (!received) return;
            setActivity(
              event.kind === 'reasoning'
                ? 'CLI передал опубликованное рассуждение'
                : event.kind === 'search'
                  ? 'CLI сообщил о поиске источников'
                  : event.kind === 'answer'
                    ? 'CLI передал ответ'
                    : 'Получено событие CLI',
            );
            const previous = reasoningStages().find(
              (item) => item.id === received.stage.logicalStageId,
            );
            if (event.reasoning && previous) {
              recordTrace(
                mergeLiveReasoning(previous, event.reasoning, event.elapsedMs, received.callIndex),
              );
            }
          }),
        ),
        {
          signal: runController.signal,
          repetitions: options.repetitions(),
          sourcePort: port.nativeSourcePort,
          sourceGrants: options.sourceGrants(),
          onProtocol: (protocol) =>
            new Promise<void>((resolve, reject) => {
              const signal = runController.signal;
              if (signal.aborted) {
                reject(
                  signal.reason instanceof Error
                    ? signal.reason
                    : new Error('Эксперимент остановлен', { cause: signal.reason }),
                );
                return;
              }
              const cleanup = () => {
                signal.removeEventListener('abort', abort);
                acceptProtocol = undefined;
                setPendingProtocol(undefined);
              };
              const abort = () => {
                cleanup();
                reject(
                  signal.reason instanceof Error
                    ? signal.reason
                    : new Error('Эксперимент остановлен', { cause: signal.reason }),
                );
              };
              signal.addEventListener('abort', abort, { once: true });
              acceptProtocol = () => {
                cleanup();
                executionState.setRunStatus('running');
                resolve();
              };
              executionState.setRunStatus('waitingForProtocol');
              setPendingProtocol(protocol);
              setActivity('Проверьте критерии и условия до создания персон.');
            }),
          onExecution: (stage) => {
            if (disposed) return;
            executionState.record(stage);
            const existing = reasoningStages().find((item) => item.id === stage.logicalStageId);
            if (existing) recordTrace({ ...existing, lifecycle: stage });
          },
          onTrace: (trace) => {
            if (!disposed) recordTrace(trace);
          },
          onProgress: (event) => {
            if (disposed) return;
            if (
              event.stage !== 'complete' &&
              !reasoningStages().some((item) => item.id === event.id)
            ) {
              const lifecycle = executionState
                .execution()
                .find((item) => item.logicalStageId === event.id);
              recordTrace({
                id: event.id,
                stage: event.stage,
                person: event.person,
                label: event.message,
                attempts: [],
                ...(lifecycle ? { lifecycle } : {}),
                trace: {
                  provider: options.settings().components[event.stage],
                  reasoningMs: 0,
                  wallTimeMs: 0,
                  terminalMs: 0,
                  summary: '',
                  reasoning: [],
                },
              });
            }
            setEvents((items) => [...items, event]);
          },
        },
      );
      if (disposed) return;
      const failure =
        output.execution?.stages
          .slice()
          .reverse()
          .find((stage) => stage.status === 'failed' || stage.status === 'cancelled')?.error ?? '';
      executionState.finish(output.status === 'complete' ? 'completed' : output.status, failure);
      setResult(output);
      setScreen('result');
      options.onCompleted();
      try {
        setSavedPath(await port.saveExperiment(JSON.parse(exportExperiment(output))));
      } catch {
        options.reportError('Результат готов, но не удалось сохранить его на диск. Скачайте JSON.');
      }
    } catch (failure) {
      if (!returningToInput && !disposed) {
        const message = failure instanceof Error ? failure.message : String(failure);
        executionState.finish(runController.signal.aborted ? 'cancelled' : 'failed', message);
        setActivity(runController.signal.aborted ? 'Эксперимент остановлен' : 'Ошибка этапа');
        const notification = runController.signal.aborted
          ? `Выполнение остановлено в PROTEUS. ${message} Можно вернуться к вводу и запустить заново.`
          : String(failure);
        setFailureNotification(notification);
        options.reportError(notification);
      }
    } finally {
      setRunning(false);
      acceptProtocol = undefined;
      setPendingProtocol(undefined);
      if (returningToInput) {
        executionState.finish('idle');
        setScreen('start');
        setActivity('Ожидаем запуск');
        options.reportError('');
        returningToInput = false;
      }
    }
  };
  const progress = () => events().at(-1)?.completed ?? 0;
  const total = () => events().at(-1)?.total ?? 8 + options.repetitions() * 5;

  const stop = () => {
    if (!running()) return;
    executionState.setRunStatus('stopping');
    controller?.abort();
    setActivity('Останавливаем CLI…');
  };
  return {
    execution: executionState.execution,
    runStatus: executionState.runStatus,
    activeStageId: executionState.activeStageId,
    failedStageId: executionState.failedStageId,
    displayStage: executionState.displayStage,
    elapsedMs: executionState.elapsedMs,
    lastCliEventAgeMs: executionState.lastCliEventAgeMs,
    failureReason: executionState.failureReason,
    failureNotification,
    screen,
    setScreen,
    available,
    readinessError,
    pendingProtocol,
    events,
    result,
    running,
    savedPath,
    activity,
    reasoningStages,
    progress,
    total,
    start,
    stop,
    approveProtocol: () => acceptProtocol?.(),
    returnToInput: () => {
      if (!pendingProtocol()) return;
      returningToInput = true;
      controller?.abort();
    },
    download: () =>
      port.downloadFile(exportExperiment(result()!), 'application/json', 'proteus-experiment.json'),
  };
}
export type ExperimentRunModel = ReturnType<typeof createExperimentRun>;
