import { invoke } from '@tauri-apps/api/core';
import {
  decodeCorpusSnapshot,
  decodeIngestionBindingRequest,
  decodeIngestionBudget,
  decodeIngestionJob,
  decodeIngestionPreview,
  decodeIngestionSelection,
  decodeSourceBinding,
} from '@proteus-int/contracts';
import type { IngestionPort, IngestionJob, CorpusPreparationPort } from '@proteus-int/contracts';
import { isDesktop } from './window-controls';
import { validateIngestionSourceHandle } from './ingestion-input';

interface Cancellation {
  readonly operationId: string;
  readonly jobId?: string;
}
async function confirmCancellation(operation: Cancellation, received: boolean): Promise<void> {
  try {
    // The first cancellation can arrive before native registers the operation.
    await invoke<void>('cancel_ingestion_operation', { operationId: operation.operationId });
    if (received && operation.jobId) {
      const stopped = jobResponse(
        await invoke<unknown>('cancel_ingestion_job', { jobId: operation.jobId }),
        operation.jobId,
      );
      if (stopped.status !== 'cancelled') throw new Error('Импорт не подтвердил остановку.');
    }
  } catch (cause) {
    throw new Error('Не удалось подтвердить отмену импорта. Проверьте его состояние.', { cause });
  }
}
async function request<T>(
  command: string,
  args: Record<string, unknown>,
  decode: (value: unknown) => T,
  signal?: AbortSignal,
  operation?: Cancellation,
  discard?: (value: unknown) => Promise<void>,
): Promise<T> {
  if (!isDesktop()) throw new Error('Импорт доступен в настольном приложении PROTEUS.');
  signal?.throwIfAborted();
  let earlyCancellation: Promise<void> | undefined;
  let received = false;
  let decoded = false;
  let receivedValue: unknown;
  let result: T;
  const cancel = () => {
    // A failed early attempt is retried and checked after the native response;
    // it must not become an unhandled rejection in an event listener.
    if (operation)
      earlyCancellation ??= invoke<void>('cancel_ingestion_operation', {
        operationId: operation.operationId,
      }).catch(() => undefined);
  };
  signal?.addEventListener('abort', cancel, { once: true });
  try {
    const value = await invoke<unknown>(command, args);
    received = true;
    receivedValue = value;
    signal?.throwIfAborted();
    result = decode(value);
    decoded = true;
  } catch (error) {
    signal?.throwIfAborted();
    throw error;
  } finally {
    signal?.removeEventListener('abort', cancel);
    await earlyCancellation;
    try {
      if (operation && (signal?.aborted || (received && !decoded && operation.jobId)))
        await confirmCancellation(operation, received);
    } finally {
      if (received && (!decoded || signal?.aborted)) await discard?.(receivedValue);
    }
  }
  signal?.throwIfAborted();
  return result;
}
async function discardUrlPreview(value: unknown): Promise<void> {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return;
  const handle = (value as Record<string, unknown>).sourceHandle;
  if (typeof handle !== 'string' || !/^url:[\w-]{32}$/.test(handle)) return;
  await invoke<void>('release_ingestion_source', { sourceHandle: handle });
}
function jobId(value: string): string {
  if (!value.trim() || new TextEncoder().encode(value).length > 128)
    throw new Error('Неверный идентификатор импорта.');
  return value;
}
function jobResponse(value: unknown, expectedId: string): IngestionJob {
  const result = decodeIngestionJob(value);
  if (result.id !== expectedId) throw new Error('Ответ относится к другому импорту.');
  return result;
}
function jobAction(
  action: 'get' | 'pause' | 'cancel' | 'resume' | 'retry',
  id: string,
  signal?: AbortSignal,
): Promise<IngestionJob> {
  return request(
    `${action}_ingestion_job`,
    { jobId: jobId(id) },
    (value) => jobResponse(value, id),
    signal,
    action === 'resume' || action === 'retry' ? { operationId: id, jobId: id } : undefined,
  );
}

// Abort governs this request only. Once start/resume/retry has returned, the
// durable job is controlled explicitly by pause/cancel. Metadata reads and the
// atomic binding write suppress late delivery without rolling back native state.
export const nativeIngestion: IngestionPort & CorpusPreparationPort = {
  async previewUrl(selectedUrl, budget, signal) {
    const url = new URL(selectedUrl);
    if (
      selectedUrl.trim() !== selectedUrl ||
      new TextEncoder().encode(selectedUrl).length > 8192 ||
      [...selectedUrl].some((char) => char.charCodeAt(0) < 32 || char.charCodeAt(0) === 127) ||
      !['https:', 'http:'].includes(url.protocol) ||
      url.username ||
      url.password ||
      url.port
    )
      throw new Error('Выберите публичную HTTP(S)-страницу без учётных данных.');
    const operationId = crypto.randomUUID();
    return request(
      'preview_ingestion_url',
      { request: { operationId, selectedUrl, budget: decodeIngestionBudget(budget) } },
      (value) => {
        const result = decodeIngestionPreview(value);
        const resource = result.resources[0];
        if (
          !validateIngestionSourceHandle(result.sourceHandle).startsWith('url:') ||
          !['selectedUrlHtml', 'selectedUrlText', 'selectedUrlMarkdown'].includes(
            result.sourceFormat,
          ) ||
          result.resources.length !== 1 ||
          resource?.estimatedDocuments !== 1 ||
          resource.authorIds.length !== 0
        )
          throw new Error('Предпросмотр не соответствует выбранному URL.');
        return result;
      },
      signal,
      { operationId },
      discardUrlPreview,
    );
  },
  async previewSavedPage(handle, contentType, budget, signal) {
    const sourceHandle = validateIngestionSourceHandle(handle);
    const sourceFormat =
      contentType === 'text/html'
        ? 'savedPageHtml'
        : contentType === 'text/plain'
          ? 'savedPageText'
          : contentType === 'text/markdown'
            ? 'savedPageMarkdown'
            : undefined;
    if (!sourceFormat) throw new Error('Неподдерживаемый формат сохранённой страницы.');
    const operationId = crypto.randomUUID();
    return request(
      'preview_ingestion_page',
      {
        request: { operationId, sourceHandle, contentType, budget: decodeIngestionBudget(budget) },
      },
      (value) => {
        const result = decodeIngestionPreview(value);
        const resource = result.resources[0];
        if (
          result.sourceHandle !== sourceHandle ||
          result.sourceFormat !== sourceFormat ||
          result.resources.length !== 1 ||
          resource?.estimatedDocuments !== 1 ||
          resource.authorIds.length !== 0
        )
          throw new Error('Предпросмотр не соответствует выбранной странице.');
        return result;
      },
      signal,
      { operationId },
    );
  },
  async prepare(id, maxDurationMs, signal) {
    const selectedJob = jobId(id);
    if (!Number.isSafeInteger(maxDurationMs) || maxDurationMs < 1 || maxDurationMs > 3600000)
      throw new Error('Неверный бюджет времени подготовки.');
    const operationId = crypto.randomUUID();
    return request(
      'prepare_ingestion_job',
      { request: { schemaVersion: 1, operationId, jobId: selectedJob, maxDurationMs } },
      (value) => {
        const result = decodeCorpusSnapshot(value);
        if (
          result.corpusId !== `corpus:${selectedJob}` ||
          result.parentSnapshotId === null ||
          !result.splitVersion.startsWith('episodeSplit:1:')
        )
          throw new Error('Ответ не является подготовленным снимком выбранного импорта.');
        return result;
      },
      signal,
      { operationId },
    );
  },
  async createBinding(input, signal) {
    const selected = decodeIngestionBindingRequest(input);
    return request(
      'create_ingestion_binding',
      { request: selected },
      (value) => {
        const binding = decodeSourceBinding(value);
        for (const key of [
          'personaId',
          'resourceId',
          'role',
          'targetAuthorId',
          'allowedContextAuthorIds',
          'allowUnknownContext',
          'period',
        ] as const) {
          if (JSON.stringify(binding[key]) !== JSON.stringify(selected[key]))
            throw new Error('Привязка не соответствует выбранному источнику и автору.');
        }
        return binding;
      },
      signal,
    );
  },
  async preview(handle, budget, signal) {
    const sourceHandle = validateIngestionSourceHandle(handle);
    const operationId = crypto.randomUUID();
    return request(
      'preview_ingestion_source',
      { request: { operationId, sourceHandle, budget: decodeIngestionBudget(budget) } },
      (value) => {
        const result = decodeIngestionPreview(value);
        if (result.sourceHandle !== sourceHandle)
          throw new Error('Предпросмотр относится к другому источнику.');
        return result;
      },
      signal,
      { operationId },
    );
  },
  async start(selection, budget, signal) {
    const operationId = crypto.randomUUID();
    const selected = decodeIngestionSelection(selection);
    const limits = decodeIngestionBudget(budget);
    return request(
      'start_ingestion_job',
      { request: { operationId, selection: selected, budget: limits } },
      (value) => {
        const job = jobResponse(value, operationId);
        if (
          JSON.stringify(job.selection) !== JSON.stringify(selected) ||
          JSON.stringify(job.budget) !== JSON.stringify(limits)
        )
          throw new Error('Импорт не соответствует выбранному источнику и бюджету.');
        return job;
      },
      signal,
      { operationId, jobId: operationId },
    );
  },
  async getJob(id, signal) {
    return jobAction('get', id, signal);
  },
  async pause(id) {
    return jobAction('pause', id);
  },
  async cancel(id) {
    return jobAction('cancel', id);
  },
  async resume(id, signal) {
    return jobAction('resume', id, signal);
  },
  async retry(id, signal) {
    return jobAction('retry', id, signal);
  },
  async publish(id, signal) {
    const operationId = crypto.randomUUID();
    return request(
      'publish_ingestion_job',
      { operationId, jobId: jobId(id) },
      decodeCorpusSnapshot,
      signal,
      { operationId },
    );
  },
};
export async function restoreIngestionSource(
  id: string,
  sourceHandle: string,
  signal?: AbortSignal,
): Promise<IngestionJob> {
  const operationId = crypto.randomUUID();
  return request(
    'restore_ingestion_source',
    {
      request: {
        operationId,
        jobId: jobId(id),
        sourceHandle: validateIngestionSourceHandle(sourceHandle),
      },
    },
    (value) => jobResponse(value, id),
    signal,
    { operationId },
  );
}
