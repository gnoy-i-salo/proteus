import type * as SolidClient from 'solid-js';
import { createRoot } from 'solid-js';
import { expect, it, vi } from 'vitest';
import type {
  IngestionBindingRequest,
  IngestionBudget,
  IngestionJob,
  IngestionPort,
  CorpusPreparationPort,
  SourceBinding,
  CorpusSnapshot,
} from '@proteus-int/contracts';
import { createSourceImport } from './source-import';
vi.mock('solid-js', () => vi.importActual<typeof SolidClient>('solid-js/dist/solid.js'));
const budget: IngestionBudget = {
  maxInputBytes: 1000000,
  maxExpandedBytes: 2000000,
  maxDocuments: 1000,
  maxBatchDocuments: 100,
  maxBatchBytes: 100000,
  maxDurationMs: 30000,
};
const request: IngestionBindingRequest = {
  previewId: 'preview',
  resourceId: 'chat',
  personaId: 'persona',
  role: 'own',
  targetAuthorId: 'author',
  allowedContextAuthorIds: [],
  allowUnknownContext: false,
  period: { from: null, to: null, includeUndated: false },
  allowModelProcessing: false,
  allowExport: false,
};
const binding: SourceBinding = {
  schemaVersion: 1,
  id: 'binding',
  personaId: 'persona',
  resourceId: 'chat',
  provider: 'telegram',
  accountId: null,
  grantId: 'grant',
  policyVersion: 'ingestionBinding:1',
  role: 'own',
  targetAuthorId: 'author',
  allowedContextAuthorIds: [],
  allowUnknownContext: false,
  period: request.period,
};
const job: IngestionJob = {
  schemaVersion: 1,
  id: 'job',
  selection: { previewId: 'preview', resourceIds: ['chat'], bindings: [binding] },
  status: 'running',
  phase: 'reading',
  budget,
  checkpoint: {
    adapterVersion: 'telegram:1',
    cursor: null,
    committedBatch: 0,
    stagingGeneration: 1,
  },
  coverage: {
    loadedDocuments: 0,
    indexedDocuments: 0,
    analyzedDocuments: 0,
    usedDocuments: 0,
    totalDocuments: null,
    loadedBytes: 0,
    complete: false,
    limitations: [],
  },
  publishedSnapshotId: null,
  error: null,
};
function setup() {
  const port = {
    createBinding: vi.fn<IngestionPort['createBinding']>().mockResolvedValue(binding),
    start: vi.fn<IngestionPort['start']>().mockResolvedValue(job),
    getJob: vi.fn<IngestionPort['getJob']>().mockResolvedValue(job),
    cancel: vi.fn<IngestionPort['cancel']>().mockResolvedValue({ ...job, status: 'cancelled' }),
    pause: vi.fn<IngestionPort['pause']>().mockResolvedValue({ ...job, status: 'paused' }),
    resume: vi.fn<IngestionPort['resume']>().mockResolvedValue(job),
    retry: vi.fn<IngestionPort['retry']>().mockResolvedValue(job),
    publish: vi.fn<IngestionPort['publish']>(),
    prepare: vi.fn<CorpusPreparationPort['prepare']>(),
    preview: vi.fn<IngestionPort['preview']>(),
    previewUrl: vi.fn<IngestionPort['previewUrl']>(),
    previewSavedPage: vi.fn<IngestionPort['previewSavedPage']>(),
  };
  let dispose!: () => void;
  const onCleanupError = vi.fn();
  const model = createRoot((stop) => {
    dispose = stop;
    return createSourceImport({ port, onCleanupError });
  });
  return { model, port, dispose, onCleanupError };
}
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}
it('creates only explicitly selected permissions, then starts the detached native selection', async () => {
  const { model, port, dispose } = setup();
  const selection = structuredClone(request);
  const limits = structuredClone(budget);
  const pending = model.start(selection, limits);
  Object.assign(selection.period, { includeUndated: true });
  Object.assign(limits, { maxDocuments: 999 });
  await pending;
  expect(port.createBinding).toHaveBeenCalledWith(request, expect.any(AbortSignal));
  expect(port.start).toHaveBeenCalledWith(job.selection, budget, expect.any(AbortSignal));
  expect(model.job()).toEqual(job);
  expect(model.snapshot()).toBeUndefined();
  dispose();
});
it('does not import after cancellation while a binding is being created', async () => {
  const { model, port, dispose } = setup();
  const response = deferred<SourceBinding>();
  port.createBinding.mockReturnValueOnce(response.promise);
  const pending = model.start(request, budget);
  await model.cancel();
  response.resolve(binding);
  await pending;
  expect(port.start).not.toHaveBeenCalled();
  expect(model.job()).toBeUndefined();
  dispose();
});
it('cancels a durable job arriving after screen disposal', async () => {
  const { model, port, dispose } = setup();
  const response = deferred<IngestionJob>();
  port.start.mockReturnValueOnce(response.promise);
  const pending = model.start(request, budget);
  await vi.waitFor(() => expect(port.start).toHaveBeenCalledOnce());
  dispose();
  response.resolve(job);
  await pending;
  expect(port.cancel).toHaveBeenCalledExactlyOnceWith('job');
  expect(model.job()).toBeUndefined();
});
it('prepares only completed native jobs and keeps real coverage without marking model study', async () => {
  const { model, port, dispose } = setup();
  await model.start(request, budget);
  await model.prepare();
  expect(port.prepare).not.toHaveBeenCalled();
  const completed: IngestionJob = {
    ...job,
    status: 'completed',
    coverage: { ...job.coverage, loadedDocuments: 30, complete: true },
  };
  port.getJob.mockResolvedValueOnce(completed);
  await model.refresh();
  const snapshot: CorpusSnapshot = {
    schemaVersion: 1,
    id: 'prepared',
    corpusId: 'corpus:job',
    parentSnapshotId: 'import:job',
    createdAt: '2026-11-09T00:00:00.000Z',
    bindingIds: ['binding'],
    documentRefs: [],
    episodeIds: ['episode'],
    splitVersion: 'episodeSplit:1:test',
    generation: 2,
    coverage: { ...completed.coverage, indexedDocuments: 30 },
  };
  port.prepare.mockResolvedValueOnce(snapshot);
  await model.prepare();
  expect(port.prepare).toHaveBeenCalledWith('job', 30000, expect.any(AbortSignal));
  expect(model.snapshot()).toEqual(snapshot);
  expect(model.snapshot()?.coverage.analyzedDocuments).toBe(0);
  dispose();
});
it('does not let a late refresh overwrite an explicit cancellation', async () => {
  const { model, port, dispose } = setup();
  await model.start(request, budget);
  const response = deferred<IngestionJob>();
  port.getJob.mockReturnValueOnce(response.promise);
  const pending = model.refresh();
  await model.cancel();
  response.resolve(job);
  await pending;
  expect(model.job()?.status).toBe('cancelled');
  dispose();
});
it('reports cleanup failure for a late job instead of silently leaving native work running', async () => {
  const { model, port, dispose, onCleanupError } = setup();
  const response = deferred<IngestionJob>();
  port.start.mockReturnValueOnce(response.promise);
  const pending = model.start(request, budget);
  await vi.waitFor(() => expect(port.start).toHaveBeenCalledOnce());
  dispose();
  const failure = new Error('Cancellation failed');
  port.cancel.mockRejectedValueOnce(failure);
  response.resolve(job);
  await pending;
  expect(onCleanupError).toHaveBeenCalledExactlyOnceWith(failure);
});
it('refreshes running progress automatically and stops polling once paused', async () => {
  vi.useFakeTimers();
  const { model, port, dispose } = setup();
  try {
    await model.start(request, budget);
    await vi.advanceTimersByTimeAsync(1000);
    expect(port.getJob).toHaveBeenCalledExactlyOnceWith('job', expect.any(AbortSignal));
    await model.pause();
    await vi.advanceTimersByTimeAsync(3000);
    expect(port.getJob).toHaveBeenCalledOnce();
    expect(model.job()?.status).toBe('paused');
  } finally {
    dispose();
    vi.useRealTimers();
  }
});
it('surfaces an adapter failure to confirm cancellation after disposal', async () => {
  const { model, port, dispose, onCleanupError } = setup();
  let reject!: (cause: unknown) => void;
  port.start.mockReturnValueOnce(
    new Promise((_resolve, fail) => {
      reject = fail;
    }),
  );
  const pending = model.start(request, budget);
  await vi.waitFor(() => expect(port.start).toHaveBeenCalledOnce());
  dispose();
  const failure = new Error('Не удалось подтвердить отмену импорта. Проверьте его состояние.');
  reject(failure);
  await pending;
  expect(onCleanupError).toHaveBeenCalledExactlyOnceWith(failure);
});
it('keeps denied bindings out of the job and displays the native failure', async () => {
  const { model, port, dispose } = setup();
  port.createBinding.mockRejectedValueOnce(new Error('Permission denied'));
  await model.start(request, budget);
  expect(port.start).not.toHaveBeenCalled();
  expect(model.error()).toBe('Permission denied');
  expect(model.job()).toBeUndefined();
  dispose();
});
