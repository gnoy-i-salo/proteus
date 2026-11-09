import { batch, createSignal, onCleanup } from 'solid-js';
import type { ExperimentInput } from '@proteus-int/contracts';
import { prepareStoredResult, readStoredResult } from '@proteus-int/pipeline';
import { listSavedExperiments, readSavedExperiment } from '../../../shared/lib/experiment-history';
import type { SavedExperimentPage } from '../../../shared/lib/experiment-history';

export interface ExperimentHistoryPort {
  readonly list: typeof listSavedExperiments;
  readonly read: typeof readSavedExperiment;
}
export interface OpenedExperiment<T> {
  readonly id: string;
  readonly sourceText: string;
  readonly result: T;
}

export function createExperimentHistory<T>(
  decode: (text: string) => T,
  port: ExperimentHistoryPort = { list: listSavedExperiments, read: readSavedExperiment },
) {
  const [page, setPage] = createSignal<SavedExperimentPage>();
  const [offset, setOffset] = createSignal(0);
  const [loading, setLoading] = createSignal(false);
  const [listError, setListError] = createSignal('');
  const [openingId, setOpeningId] = createSignal<string>();
  const [openError, setOpenError] = createSignal('');
  const [opened, setOpened] = createSignal<OpenedExperiment<T>>();
  let listing: AbortController | undefined;
  let reading: AbortController | undefined;
  let disposed = false;
  const active = (request: AbortController) => !disposed && !request.signal.aborted;
  const cancelRead = () => {
    reading?.abort();
    reading = undefined;
    setOpeningId(undefined);
  };
  const cancel = () => {
    listing?.abort();
    listing = undefined;
    batch(() => {
      setLoading(false);
      cancelRead();
    });
  };
  onCleanup(() => {
    disposed = true;
    cancel();
  });
  const load = async (requestedOffset: number) => {
    if (disposed) return;
    listing?.abort();
    const request = new AbortController();
    listing = request;
    batch(() => {
      setLoading(true);
      setListError('');
    });
    try {
      const result = await port.list(requestedOffset, 50, request.signal);
      if (!active(request)) return;
      // Pages replace each other. The bounded native directory scan is not a
      // stable cursor, so merging pages would imply unsupported completeness.
      batch(() => {
        setPage(result);
        setOffset(requestedOffset);
      });
    } catch {
      if (active(request))
        setListError('Не удалось прочитать историю. Попробуйте обновить список.');
    } finally {
      if (active(request)) {
        listing = undefined;
        setLoading(false);
      }
    }
  };
  const open = async (id: string) => {
    if (disposed) return;
    cancelRead();
    setOpenError('');
    if (!page()?.entries.some((entry) => entry.id === id && entry.ok)) {
      setOpenError('Сохранённый результат недоступен. Обновите список.');
      return;
    }
    const request = new AbortController();
    reading = request;
    setOpeningId(id);
    try {
      const source = await port.read(id, request.signal);
      if (!active(request)) return;
      const result = decode(source.text);
      if (!active(request)) return;
      setOpened({ id: source.id, sourceText: source.text, result });
    } catch {
      if (active(request))
        setOpenError(
          'Не удалось открыть результат: файл недоступен, повреждён или имеет неподдерживаемую версию.',
        );
    } finally {
      if (active(request)) {
        reading = undefined;
        setOpeningId(undefined);
      }
    }
  };
  return {
    page,
    offset,
    loading,
    listError,
    openingId,
    openError,
    opened,
    refresh: () => load(0),
    nextPage: async () => {
      const next = page()?.nextOffset;
      if (!loading() && next !== undefined && next !== null) await load(next);
    },
    previousPage: async () => {
      if (!loading() && offset() > 0) await load(Math.max(0, offset() - 50));
    },
    open,
    cancel,
    clearOpened: () => {
      cancelRead();
      setOpened(undefined);
      setOpenError('');
    },
  };
}
export type ExperimentHistoryModel<T> = ReturnType<typeof createExperimentHistory<T>>;

export function createSavedExperimentHistory() {
  return createExperimentHistory((text) => prepareStoredResult(readStoredResult(text)));
}

export interface HistoryReusePort {
  readonly running: () => boolean;
  readonly clearSourceGrants: () => void;
  readonly applyInput: (value: ExperimentInput) => void;
  readonly setRepetitions: (value: 1 | 3) => void;
  readonly onApplied: () => void;
}
export function reuseHistoryConditions(
  input: ExperimentInput,
  repetitions: 1 | 3,
  port: HistoryReusePort,
): boolean {
  if (port.running()) return false;
  batch(() => {
    port.clearSourceGrants();
    port.applyInput(input);
    port.setRepetitions(repetitions);
    port.onApplied();
  });
  return true;
}
