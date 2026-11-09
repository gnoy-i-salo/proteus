import { createSignal, onCleanup } from 'solid-js';
import { decodeIngestionBudget } from '@proteus-int/contracts';
import type { IngestionBudget, IngestionPort, IngestionPreview } from '@proteus-int/contracts';
import type { SelectedIngestionInput } from '../../../shared/lib/ingestion-input';
export type SourceFileKind = 'telegramArchive' | 'savedHtml' | 'savedText' | 'savedMarkdown';
export interface SourcePreviewPort extends Pick<
  IngestionPort,
  'preview' | 'previewSavedPage' | 'previewUrl'
> {
  pick(): Promise<SelectedIngestionInput | null>;
  release(handle: string): Promise<void>;
}
export function createSourcePreview(options: {
  readonly port: SourcePreviewPort;
  readonly budget: () => IngestionBudget;
  readonly onCleanupError: (error: unknown) => void;
}) {
  const [preview, setPreview] = createSignal<IngestionPreview>();
  const [selected, setSelected] = createSignal<SelectedIngestionInput>();
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal('');
  const handles = new Set<string>();
  const releasing = new Map<string, Promise<void>>();
  let currentHandle: string | undefined;
  let controller: AbortController | undefined;
  let revision = 0;
  let disposed = false;
  function release(handle: string): Promise<void> {
    const prior = releasing.get(handle);
    if (prior) return prior;
    const pending = options.port
      .release(handle)
      .then(() => {
        handles.delete(handle);
      })
      .finally(() => {
        releasing.delete(handle);
      });
    releasing.set(handle, pending);
    return pending;
  }
  function releaseAll() {
    for (const handle of handles) void release(handle).catch(options.onCleanupError);
    currentHandle = undefined;
  }
  function cancel() {
    revision++;
    controller?.abort();
    controller = undefined;
    releaseAll();
    if (!disposed) {
      setBusy(false);
      setPreview(undefined);
      setSelected(undefined);
    }
  }
  onCleanup(() => {
    disposed = true;
    cancel();
  });
  function begin() {
    controller?.abort();
    controller = new AbortController();
    const id = ++revision;
    setBusy(true);
    setError('');
    return { signal: controller.signal, current: () => !disposed && id === revision };
  }
  async function adopt(handle: string, current: () => boolean): Promise<boolean> {
    handles.add(handle);
    if (!current()) {
      await release(handle);
      return false;
    }
    for (const old of handles) {
      if (old !== handle) await release(old);
    }
    if (!current()) {
      await release(handle);
      return false;
    }
    currentHandle = handle;
    setPreview(undefined);
    return true;
  }
  const message = (cause: unknown) =>
    cause instanceof Error ? cause.message : 'Не удалось прочитать выбранный источник.';
  async function choose(kind: SourceFileKind) {
    if (disposed) return;
    const operation = begin();
    let incoming: string | undefined;
    try {
      const budget = decodeIngestionBudget(options.budget());
      const input = await options.port.pick();
      if (!input) return;
      incoming = input.sourceHandle;
      handles.add(incoming);
      if (!operation.current()) {
        await release(incoming);
        return;
      }
      if (input.inputBytes > budget.maxInputBytes)
        throw new Error('Файл превышает выбранный лимит импорта.');
      if (!(await adopt(incoming, operation.current))) return;
      setSelected(structuredClone(input));
      const result =
        kind === 'telegramArchive'
          ? await options.port.preview(incoming, budget, operation.signal)
          : await options.port.previewSavedPage(
              incoming,
              kind === 'savedHtml'
                ? 'text/html'
                : kind === 'savedText'
                  ? 'text/plain'
                  : 'text/markdown',
              budget,
              operation.signal,
            );
      if (operation.current()) setPreview(structuredClone(result));
    } catch (cause) {
      if (operation.current()) setError(message(cause));
    } finally {
      if (incoming && incoming !== currentHandle && handles.has(incoming))
        void release(incoming).catch(options.onCleanupError);
      if (operation.current()) setBusy(false);
    }
  }
  async function previewUrl(url: string) {
    if (disposed) return;
    const operation = begin();
    setPreview(undefined);
    setSelected(undefined);
    let incoming: string | undefined;
    try {
      const result = await options.port.previewUrl(
        url,
        decodeIngestionBudget(options.budget()),
        operation.signal,
      );
      incoming = result.sourceHandle;
      if (!(await adopt(incoming, operation.current))) return;
      setSelected(undefined);
      setPreview(structuredClone(result));
    } catch (cause) {
      if (operation.current()) setError(message(cause));
    } finally {
      if (incoming && incoming !== currentHandle && handles.has(incoming))
        void release(incoming).catch(options.onCleanupError);
      if (operation.current()) setBusy(false);
    }
  }
  return { preview, selected, busy, error, choose, previewUrl, cancel };
}
