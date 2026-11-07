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
  const [screen, setScreen] = createSignal<'start' | 'progress' | 'result'>('start');
  const [available, setAvailable] = createSignal<CliStatus[]>([]);
  const [pendingProtocol, setPendingProtocol] = createSignal<ExperimentProtocol>();
  let acceptProtocol: (() => void) | undefined;
  const [events, setEvents] = createSignal<ProgressEvent[]>([]);
  const [result, setResult] = createSignal<ExperimentResult>();
  const [running, setRunning] = createSignal(false);
  const [savedPath, setSavedPath] = createSignal('');
  const [activity, setActivity] = createSignal('Ожидаем запуск');
  const [reasoningStages, setReasoningStages] = createSignal<StageTrace[]>([]);
  const recordTrace = (next: StageTrace) =>
    setReasoningStages((items) => {
      const index = items.findIndex((item) => item.id === next.id);
      return index < 0 ? [...items, next] : items.map((item, i) => (i === index ? next : item));
    });
  let controller: AbortController | undefined;
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
  onCleanup(() => controller?.abort());
  const start = async () => {
    options.reportError('');
    setSavedPath('');
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
    controller = new AbortController();
    setRunning(true);
    setEvents([]);
    options.onStarted();
    setReasoningStages([]);
    setPendingProtocol(undefined);
    setResult(undefined);
    setScreen('progress');
    try {
      for (const grant of options.sourceGrants().flat()) {
        controller.signal.throwIfAborted();
        await port.resumeSourceGrant(grant.id);
      }
      controller.signal.throwIfAborted();
      const output = await port.runExperiment(
        input,
        options.settings(),
        port.cliProviders((event) =>
          untrack(() => {
            setActivity(
              `${event.kind === 'reasoning' ? 'Модель рассуждает' : event.kind === 'search' ? 'Поиск источников' : event.kind === 'answer' ? 'Ответ получен' : 'CLI работает'} · ${(event.elapsedMs / 1000).toFixed(1)} с`,
            );
            const current = events().at(-1);
            if (event.reasoning && current && current.stage !== 'complete') {
              const previous = reasoningStages().find((item) => item.id === current.id);
              if (previous) {
                recordTrace(mergeLiveReasoning(previous, event.reasoning, event.elapsedMs));
              }
            }
          }),
        ),
        {
          signal: controller.signal,
          repetitions: options.repetitions(),
          sourcePort: port.nativeSourcePort,
          sourceGrants: options.sourceGrants(),
          onProtocol: (protocol) =>
            new Promise<void>((resolve, reject) => {
              const signal = controller!.signal;
              if (signal.aborted) {
                reject(signal.reason);
                return;
              }
              const cleanup = () => {
                signal.removeEventListener('abort', abort);
                acceptProtocol = undefined;
                setPendingProtocol(undefined);
              };
              const abort = () => {
                cleanup();
                reject(signal.reason ?? new Error('Эксперимент остановлен'));
              };
              signal.addEventListener('abort', abort, { once: true });
              acceptProtocol = () => {
                cleanup();
                resolve();
              };
              setPendingProtocol(protocol);
              setActivity('Проверьте критерии и условия до создания персон.');
            }),
          onTrace: recordTrace,
          onProgress: (event) => {
            if (
              event.stage !== 'complete' &&
              !reasoningStages().some((item) => item.id === event.id)
            ) {
              recordTrace({
                id: event.id,
                stage: event.stage,
                person: event.person,
                label: event.message,
                attempts: [],
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
      setResult(output);
      setScreen('result');
      options.onCompleted();
      try {
        setSavedPath(await port.saveExperiment(JSON.parse(exportExperiment(output))));
      } catch {
        options.reportError('Результат готов, но не удалось сохранить его на диск. Скачайте JSON.');
      }
    } catch (failure) {
      setActivity(controller.signal.aborted ? 'Эксперимент остановлен' : 'Ошибка этапа');
      options.reportError(
        controller.signal.aborted
          ? 'Эксперимент остановлен. Можно вернуться к вводу и запустить заново.'
          : String(failure),
      );
    } finally {
      setRunning(false);
      acceptProtocol = undefined;
      setPendingProtocol(undefined);
    }
  };
  const progress = () => events().at(-1)?.completed ?? 0;
  const total = () => events().at(-1)?.total ?? 8 + options.repetitions() * 5;

  const stop = () => {
    controller?.abort();
    setActivity('Останавливаем CLI…');
  };
  return {
    screen,
    setScreen,
    available,
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
    download: () =>
      port.downloadFile(exportExperiment(result()!), 'application/json', 'proteus-experiment.json'),
  };
}
export type ExperimentRunModel = ReturnType<typeof createExperimentRun>;
