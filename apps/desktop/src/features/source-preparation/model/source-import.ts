import { createSignal, onCleanup } from 'solid-js';
import { decodeIngestionBindingRequest, decodeIngestionBudget } from '@proteus-int/contracts';
import type {
  CorpusPreparationPort,
  CorpusSnapshot,
  IngestionBindingRequest,
  IngestionBudget,
  IngestionJob,
  IngestionPort,
} from '@proteus-int/contracts';
export function createSourceImport(options: {
  readonly port: IngestionPort & CorpusPreparationPort;
  readonly onCleanupError: (error: unknown) => void;
}) {
  const [job, setJob] = createSignal<IngestionJob>();
  const [snapshot, setSnapshot] = createSignal<CorpusSnapshot>();
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal('');
  let controller: AbortController | undefined;
  let revision = 0;
  let disposed = false;
  let timer: ReturnType<typeof setTimeout> | undefined;
  const active = (value: IngestionJob | undefined) =>
    value && ['queued', 'running', 'paused'].includes(value.status);
  const message = (cause: unknown) =>
    cause instanceof Error ? cause.message : 'Не удалось изменить состояние импорта.';
  function reportError(cause: unknown) {
    if (!disposed) setError(message(cause));
  }
  function reportOperationError(operation: ReturnType<typeof begin>, cause: unknown) {
    if (operation.current()) setError(message(cause));
    else if (!(cause instanceof Error && cause.name === 'AbortError'))
      options.onCleanupError(cause);
  }
  function begin() {
    clearTimeout(timer);
    controller?.abort();
    controller = new AbortController();
    const id = ++revision;
    setBusy(true);
    setError('');
    return { signal: controller.signal, current: () => !disposed && id === revision };
  }
  function finish(operation: ReturnType<typeof begin>) {
    if (!operation.current()) return;
    setBusy(false);
    if (['queued', 'running'].includes(job()?.status ?? ''))
      timer = setTimeout(() => {
        void action('refresh').catch(reportError);
      }, 1000);
  }
  async function stopLate(value: IngestionJob) {
    if (active(value)) await options.port.cancel(value.id).catch(options.onCleanupError);
  }
  async function start(request: IngestionBindingRequest, budget: IngestionBudget) {
    if (disposed || busy() || job()) return;
    const operation = begin();
    try {
      const selection = decodeIngestionBindingRequest(request);
      const limits = decodeIngestionBudget(budget);
      const binding = await options.port.createBinding(selection, operation.signal);
      if (!operation.current()) return;
      const result = await options.port.start(
        {
          previewId: selection.previewId,
          resourceIds: [selection.resourceId],
          bindings: [binding],
        },
        limits,
        operation.signal,
      );
      if (!operation.current()) {
        await stopLate(result);
        return;
      }
      setJob(structuredClone(result));
    } catch (cause) {
      reportOperationError(operation, cause);
    } finally {
      finish(operation);
    }
  }
  async function action(kind: 'refresh' | 'pause' | 'resume' | 'retry') {
    const selected = job();
    if (disposed || busy() || !selected) return;
    const operation = begin();
    try {
      const result =
        kind === 'refresh'
          ? await options.port.getJob(selected.id, operation.signal)
          : kind === 'pause'
            ? await options.port.pause(selected.id)
            : await options.port[kind](selected.id, operation.signal);
      if (operation.current()) setJob(structuredClone(result));
      else if (kind === 'resume' || kind === 'retry') await stopLate(result);
    } catch (cause) {
      reportOperationError(operation, cause);
    } finally {
      finish(operation);
    }
  }
  async function cancel() {
    if (disposed) return;
    const selected = job();
    const operation = begin();
    try {
      if (active(selected) && selected) {
        const result = await options.port.cancel(selected.id);
        if (operation.current()) setJob(structuredClone(result));
      }
    } catch (cause) {
      reportOperationError(operation, cause);
    } finally {
      finish(operation);
    }
  }
  async function prepare() {
    const selected = job();
    if (disposed || busy() || !selected) return;
    if (selected.status !== 'completed') {
      setError('Дождитесь завершения импорта.');
      return;
    }
    const operation = begin();
    try {
      const result = await options.port.prepare(
        selected.id,
        selected.budget.maxDurationMs,
        operation.signal,
      );
      if (operation.current()) setSnapshot(structuredClone(result));
    } catch (cause) {
      reportOperationError(operation, cause);
    } finally {
      finish(operation);
    }
  }
  onCleanup(() => {
    disposed = true;
    clearTimeout(timer);
    revision++;
    controller?.abort();
    const selected = job();
    if (selected) void stopLate(selected).catch(options.onCleanupError);
  });
  return {
    reportError,
    job,
    snapshot,
    busy,
    error,
    start,
    refresh: () => action('refresh'),
    cancel,
    pause: () => action('pause'),
    resume: () => action('resume'),
    retry: () => action('retry'),
    prepare,
  };
}
