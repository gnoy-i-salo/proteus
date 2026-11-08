import { beforeEach, expect, it, vi } from 'vitest';
import type {
  CorpusSnapshot,
  IngestionBudget,
  IngestionJob,
  IngestionPreview,
  IngestionSelection,
} from '@proteus-int/contracts';
import { nativeIngestion, restoreIngestionSource } from './ingestion';

const native = vi.hoisted(() => ({ desktop: true, invoke: vi.fn() }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: native.invoke }));
vi.mock('./window-controls', () => ({ isDesktop: () => native.desktop }));
const operationId = '00000000-0000-4000-8000-000000000001';
const sourceHandle = 'A'.repeat(32);
const exact = '  é 👩🏽‍🔬\r\nисточник\t';
const budget: IngestionBudget = {
  maxInputBytes: 128 * 1024 * 1024,
  maxExpandedBytes: 512 * 1024 * 1024,
  maxDocuments: 100000,
  maxBatchDocuments: 64,
  maxBatchBytes: 1024 * 1024,
  maxDurationMs: 60000,
};
const selection: IngestionSelection = {
  previewId: 'preview:1',
  resourceIds: ['chat:1'],
  bindings: [
    {
      schemaVersion: 1,
      id: 'binding:1',
      personaId: 'persona:a',
      provider: 'telegram',
      accountId: null,
      resourceId: 'chat:1',
      grantId: 'grant:1',
      policyVersion: 'policy:1',
      role: 'own',
      targetAuthorId: 'author:1',
      allowedContextAuthorIds: ['author:2'],
      allowUnknownContext: true,
      period: { from: null, to: null, includeUndated: true },
    },
  ],
};
const preview: IngestionPreview = {
  schemaVersion: 1,
  id: selection.previewId,
  sourceHandle,
  sourceFormat: 'telegramDesktopJson',
  adapterVersion: 'telegramDesktopJson:1',
  resources: [
    { id: 'chat:1', label: exact, authorIds: ['author:1', 'author:2'], estimatedDocuments: 400 },
  ],
  limitations: [exact],
};
const job: IngestionJob = {
  schemaVersion: 1,
  id: operationId,
  selection,
  status: 'running',
  phase: 'reading',
  budget,
  checkpoint: {
    adapterVersion: preview.adapterVersion,
    cursor: exact,
    committedBatch: 2,
    stagingGeneration: 3,
  },
  coverage: {
    loadedDocuments: 400,
    indexedDocuments: 0,
    analyzedDocuments: 0,
    usedDocuments: 0,
    totalDocuments: null,
    loadedBytes: 20000,
    complete: false,
    limitations: [exact],
  },
  publishedSnapshotId: null,
  error: null,
};
const snapshot: CorpusSnapshot = {
  schemaVersion: 1,
  id: 'snapshot:1',
  corpusId: 'corpus:1',
  parentSnapshotId: null,
  createdAt: '2026-11-07T12:00:00.123456789+00:00',
  bindingIds: ['binding:1'],
  documentRefs: [{ documentId: exact, revisionId: 'revision:1' }],
  episodeIds: [],
  splitVersion: 'unpartitioned:1',
  generation: 1,
  coverage: { ...job.coverage, complete: true, totalDocuments: 400 },
};
const bindingRequest = {
  previewId: selection.previewId,
  resourceId: 'chat:1',
  personaId: 'persona:a',
  role: 'own' as const,
  targetAuthorId: 'author:1',
  allowedContextAuthorIds: ['author:2'],
  allowUnknownContext: true,
  period: { from: null, to: null, includeUndated: true },
  allowModelProcessing: false,
  allowExport: false,
};
beforeEach(() => {
  vi.restoreAllMocks();
  vi.resetAllMocks();
  native.desktop = true;
  vi.spyOn(crypto, 'randomUUID').mockReturnValue(operationId);
});

it('previews the selected native capability with bounded budget and exact source labels', async () => {
  native.invoke.mockResolvedValue(preview);
  expect(await nativeIngestion.preview(sourceHandle, budget)).toEqual(preview);
  expect(native.invoke).toHaveBeenCalledExactlyOnceWith('preview_ingestion_source', {
    request: { operationId, sourceHandle, budget },
  });
});
it.each([
  ['text/html', 'savedPageHtml'],
  ['text/plain', 'savedPageText'],
  ['text/markdown', 'savedPageMarkdown'],
] as const)(
  'previews a saved page as %s through the selected-file capability',
  async (contentType, sourceFormat) => {
    const page = {
      ...preview,
      sourceFormat,
      resources: [{ id: 'page:1', label: exact, authorIds: [], estimatedDocuments: 1 }],
    };
    native.invoke.mockResolvedValue(page);
    expect(await nativeIngestion.previewSavedPage(sourceHandle, contentType, budget)).toEqual(page);
    expect(native.invoke).toHaveBeenCalledExactlyOnceWith('preview_ingestion_page', {
      request: { operationId, sourceHandle, contentType, budget },
    });
  },
);
it('rejects unsupported saved-page formats before IPC and mismatched native source formats', async () => {
  native.invoke.mockResolvedValue(preview);
  await expect(
    nativeIngestion.previewSavedPage(sourceHandle, 'application/json' as never, budget),
  ).rejects.toThrow();
  expect(native.invoke).not.toHaveBeenCalled();
  await expect(
    nativeIngestion.previewSavedPage(sourceHandle, 'text/html', budget),
  ).rejects.toThrow();
});
it('creates a binding with independent model and export choices', async () => {
  native.invoke.mockResolvedValue(selection.bindings[0]);
  expect(await nativeIngestion.createBinding(bindingRequest)).toEqual(selection.bindings[0]);
  expect(native.invoke).toHaveBeenCalledExactlyOnceWith('create_ingestion_binding', {
    request: bindingRequest,
  });
});
it('starts a durable job and keeps checkpoint and loaded counters exact', async () => {
  native.invoke.mockResolvedValue(job);
  expect(await nativeIngestion.start(selection, budget)).toEqual(job);
  expect(native.invoke).toHaveBeenCalledExactlyOnceWith('start_ingestion_job', {
    request: { operationId, selection, budget },
  });
});
it.each(['getJob', 'pause', 'cancel', 'resume', 'retry'] as const)(
  'routes %s to native job state',
  async (method) => {
    const status = method === 'pause' ? 'paused' : method === 'cancel' ? 'cancelled' : 'running';
    const result = { ...job, status };
    native.invoke.mockResolvedValue(result);
    expect(await nativeIngestion[method](job.id)).toEqual(result);
    const action = method === 'getJob' ? 'get' : method;
    expect(native.invoke).toHaveBeenCalledExactlyOnceWith(`${action}_ingestion_job`, {
      jobId: job.id,
    });
  },
);
it('gets a strictly decoded immutable acquisition snapshot', async () => {
  native.invoke.mockResolvedValue(snapshot);
  expect(await nativeIngestion.publish(job.id)).toEqual(snapshot);
  expect(native.invoke).toHaveBeenCalledExactlyOnceWith('publish_ingestion_job', {
    operationId,
    jobId: job.id,
  });
});
it('restores a known job from a new native file selection without sending a path', async () => {
  const restored = { ...job, status: 'paused' };
  native.invoke.mockResolvedValue(restored);
  expect(await restoreIngestionSource(job.id, sourceHandle)).toEqual(restored);
  expect(native.invoke).toHaveBeenCalledExactlyOnceWith('restore_ingestion_source', {
    request: { operationId, jobId: job.id, sourceHandle },
  });
});
it('never fabricates native results in browser preview', async () => {
  native.desktop = false;
  await expect(nativeIngestion.start(selection, budget)).rejects.toThrow();
  await expect(nativeIngestion.getJob(job.id)).rejects.toThrow();
  await expect(nativeIngestion.preview(sourceHandle, budget)).rejects.toThrow();
  await expect(nativeIngestion.publish(job.id)).rejects.toThrow();
  expect(native.invoke).not.toHaveBeenCalled();
});
it('does not start native work for an already aborted request', async () => {
  const signal = AbortSignal.abort(new Error('already stopped'));
  await expect(nativeIngestion.start(selection, budget, signal)).rejects.toThrow('already stopped');
  await expect(nativeIngestion.preview(sourceHandle, budget, signal)).rejects.toThrow(
    'already stopped',
  );
  await expect(nativeIngestion.getJob(job.id, signal)).rejects.toThrow('already stopped');
  expect(native.invoke).not.toHaveBeenCalled();
});
it('rejects invalid inputs before IPC', async () => {
  await expect(nativeIngestion.preview('/private/archive.json', budget)).rejects.toThrow();
  await expect(
    nativeIngestion.start(selection, { ...budget, maxBatchDocuments: 0 }),
  ).rejects.toThrow();
  await expect(nativeIngestion.cancel('я'.repeat(65))).rejects.toThrow();
  await expect(restoreIngestionSource('', sourceHandle)).rejects.toThrow();
  expect(native.invoke).not.toHaveBeenCalled();
});
it('rejects a foreign preview handle and mismatched job identity', async () => {
  native.invoke.mockResolvedValue({ ...preview, sourceHandle: 'B'.repeat(32) });
  await expect(nativeIngestion.preview(sourceHandle, budget)).rejects.toThrow();
  native.invoke.mockResolvedValue({ ...job, id: 'another-job' });
  await expect(nativeIngestion.getJob(job.id)).rejects.toThrow();
  await expect(nativeIngestion.start(selection, budget)).rejects.toThrow();
});
it('rejects malformed native metadata and retains native failures', async () => {
  native.invoke.mockResolvedValue({ ...snapshot, schemaVersion: 2 });
  await expect(nativeIngestion.publish(job.id)).rejects.toThrow();
  native.invoke.mockResolvedValue({ ...job, coverage: { ...job.coverage, usedDocuments: 999 } });
  await expect(nativeIngestion.getJob(job.id)).rejects.toThrow();
  const failure = new Error('source revoked');
  native.invoke.mockRejectedValue(failure);
  await expect(nativeIngestion.getJob(job.id)).rejects.toBe(failure);
});

it('cancels a pending preview and retries cancellation after a late registration', async () => {
  const pending = deferred();
  const controller = new AbortController();
  native.invoke.mockImplementation(async (command: string) =>
    command === 'preview_ingestion_source' ? pending.promise : undefined,
  );
  const result = nativeIngestion.preview(sourceHandle, budget, controller.signal);
  const failure = new Error('stopped');
  const rejected = expect(result).rejects.toBe(failure);
  controller.abort(failure);
  expect(native.invoke).toHaveBeenCalledWith('cancel_ingestion_operation', { operationId });
  pending.resolve(preview);
  await rejected;
  expect(
    native.invoke.mock.calls.filter(([command]) => command === 'cancel_ingestion_operation'),
  ).toHaveLength(2);
});
it.each(['start', 'resume', 'retry'] as const)(
  'cancels durable work when a %s response arrives after abort',
  async (method) => {
    const pending = deferred();
    const controller = new AbortController();
    native.invoke.mockImplementation(async (command: string) => {
      if (command === `${method}_ingestion_job`) return pending.promise;
      if (command === 'cancel_ingestion_job') return { ...job, status: 'cancelled' };
      return undefined;
    });
    const result =
      method === 'start'
        ? nativeIngestion.start(selection, budget, controller.signal)
        : nativeIngestion[method](job.id, controller.signal);
    const failure = new Error('cancel requested');
    const rejected = expect(result).rejects.toBe(failure);
    controller.abort(failure);
    pending.resolve(job);
    await rejected;
    expect(native.invoke).toHaveBeenCalledWith('cancel_ingestion_job', { jobId: job.id });
  },
);
it('removes abort listeners after start returns, leaving subsequent control explicit', async () => {
  native.invoke.mockResolvedValue(job);
  const controller = new AbortController();
  await nativeIngestion.start(selection, budget, controller.signal);
  controller.abort();
  expect(native.invoke).toHaveBeenCalledTimes(1);
});
it('suppresses late metadata reads without cancelling the durable job', async () => {
  const pending = deferred();
  native.invoke.mockReturnValue(pending.promise);
  const controller = new AbortController();
  const result = nativeIngestion.getJob(job.id, controller.signal);
  const failure = new Error('view closed');
  const rejected = expect(result).rejects.toBe(failure);
  controller.abort(failure);
  pending.resolve(job);
  await rejected;
  expect(native.invoke).toHaveBeenCalledTimes(1);
});
it('reports a failed cancellation acknowledgement rather than claiming the worker stopped', async () => {
  const pending = deferred();
  native.invoke.mockImplementation(async (command: string) => {
    if (command === 'preview_ingestion_source') return pending.promise;
    throw new Error('cancel unavailable');
  });
  const controller = new AbortController();
  const result = nativeIngestion.preview(sourceHandle, budget, controller.signal);
  const rejected = expect(result).rejects.toThrow('Не удалось подтвердить отмену');
  controller.abort();
  pending.resolve(preview);
  await rejected;
});
it('rejects a binding response that silently changes the selected persona', async () => {
  native.invoke.mockResolvedValue({ ...selection.bindings[0], personaId: 'someone-else' });
  await expect(nativeIngestion.createBinding(bindingRequest)).rejects.toThrow();
});
it('cancels a newly started job if its returned selection differs from the request', async () => {
  native.invoke.mockImplementation((command: string) =>
    Promise.resolve(
      command === 'start_ingestion_job'
        ? { ...job, selection: { ...selection, previewId: 'other-preview' } }
        : command === 'cancel_ingestion_job'
          ? { ...job, status: 'cancelled' }
          : undefined,
    ),
  );
  await expect(nativeIngestion.start(selection, budget)).rejects.toThrow();
  expect(native.invoke).toHaveBeenCalledWith('cancel_ingestion_job', { jobId: job.id });
});

function deferred() {
  let resolve: (value: unknown) => void = () => undefined;
  const promise = new Promise<unknown>((callback) => {
    resolve = callback;
  });
  return { promise, resolve };
}

it('does not deliver a decoded job if abort happens while the request finishes cleanup', async () => {
  native.invoke.mockResolvedValue(job);
  const controller = new AbortController();
  const result = nativeIngestion.getJob(job.id, controller.signal);
  const failure = new Error('closed during cleanup');
  const rejected = expect(result).rejects.toBe(failure);
  // The native promise resolves first; the adapter still must finish its cleanup.
  await Promise.resolve();
  controller.abort(failure);
  await rejected;
});

const prepared = {
  ...snapshot,
  id: 'prepared:1',
  corpusId: 'corpus:job',
  parentSnapshotId: snapshot.id,
  episodeIds: ['episode:1'],
  splitVersion: 'episodeSplit:1:test',
  generation: 2,
};
it('prepares a completed import through native policy with no frontend source payload', async () => {
  native.invoke.mockResolvedValue(prepared);
  expect(await nativeIngestion.prepare('job', 60000)).toEqual(prepared);
  expect(native.invoke).toHaveBeenCalledExactlyOnceWith('prepare_ingestion_job', {
    request: { schemaVersion: 1, operationId, jobId: 'job', maxDurationMs: 60000 },
  });
});
it('rejects preparation budgets and identifiers before native invocation', async () => {
  for (const invalid of [0, -1, 3600001, 1.5, NaN, Infinity])
    await expect(nativeIngestion.prepare('job', invalid)).rejects.toThrow();
  await expect(nativeIngestion.prepare('', 60000)).rejects.toThrow();
  expect(native.invoke).not.toHaveBeenCalled();
});
it('rejects an unprepared or unrelated snapshot returned by preparation', async () => {
  for (const response of [
    snapshot,
    { ...prepared, corpusId: 'corpus:other' },
    { ...prepared, parentSnapshotId: null },
  ]) {
    native.invoke.mockResolvedValue(response);
    await expect(nativeIngestion.prepare('job', 60000)).rejects.toThrow();
  }
});
it('cancels preparation and suppresses late results without cancelling the completed import', async () => {
  const pending = deferred();
  native.invoke.mockImplementation((command: string) =>
    command === 'prepare_ingestion_job' ? pending.promise : Promise.resolve(undefined),
  );
  const controller = new AbortController();
  const result = nativeIngestion.prepare('job', 60000, controller.signal);
  const failure = new Error('preparation view closed');
  const rejected = expect(result).rejects.toBe(failure);
  controller.abort(failure);
  pending.resolve(prepared);
  await rejected;
  expect(native.invoke).toHaveBeenCalledWith('cancel_ingestion_operation', { operationId });
  expect(native.invoke).not.toHaveBeenCalledWith('cancel_ingestion_job', expect.anything());
});
