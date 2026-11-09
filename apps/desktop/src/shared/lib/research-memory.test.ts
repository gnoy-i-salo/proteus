import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import type {
  ModelContextRequest,
  OpenResearchSessionRequest,
  ResearchSessionDescriptor,
} from '@proteus-int/contracts';
import { openNativeResearch } from './research-memory';
import sourceFixture from './fixtures/native-wire.json';

const host = vi.hoisted(() => ({
  desktop: true,
  invoke: vi.fn<(command: string, args?: Record<string, unknown>) => Promise<unknown>>(),
}));
vi.mock('@tauri-apps/api/core', () => ({ invoke: host.invoke }));
vi.mock('./window-controls', () => ({ isDesktop: () => host.desktop }));
const request: OpenResearchSessionRequest = {
  schemaVersion: 1,
  operationId: 'operation:1',
  jobId: 'job:1',
  snapshotId: 'corpus:1',
  personaId: 'persona:1',
  mode: 'lexical',
  period: { from: null, to: null, includeUndated: true },
  recallBudget: {
    maxCalls: 5,
    maxCandidates: 10,
    maxEpisodes: 10,
    maxSourceBytes: 10000,
    maxContextTokens: 5000,
    maxDurationMs: 60000,
  },
  modelBudget: { maxCalls: 3, maxInputBytes: 10000, maxDurationMs: 60000 },
  model: { provider: 'codex', model: 'fixture-model', effort: 'medium' },
};
const descriptor: ResearchSessionDescriptor = {
  schemaVersion: 1,
  id: 'memorySession:one',
  personaId: request.personaId,
  corpusSnapshotId: request.snapshotId,
  policyVersion: 'policy:1',
  mode: 'lexical',
  indexGeneration: 1,
  expiresAt: '2026-11-09T03:01:00.000Z',
};
const context: ModelContextRequest = {
  schemaVersion: 2,
  purpose: 'research',
  manifestId: null,
  systemPrompt: 'Система\r\n',
  task: 'Задача',
  receiptIds: ['receipt:1'],
  memoryItemIds: [],
  formatAttempt: 1,
};
function retrieval(operation: 'recall' | 'readEpisode' = 'recall') {
  return {
    receipt: {
      schemaVersion: 2,
      operation,
      id: 'receipt:1',
      snapshotId: request.snapshotId,
      policyVersion: descriptor.policyVersion,
      retrievalVersion: 'nativeMemoryRetrieval:1',
      indexGeneration: 1,
      embeddingFingerprint: null,
      mode: 'lexical',
      elapsedMs: 1,
      sourceBytes: 0,
      contextTokens: 0,
      hits: [],
    },
    sources: [],
  };
}
function lease() {
  const task = context.task + '\nКонтекст native';
  const inputBytes = new TextEncoder().encode(JSON.stringify([context.systemPrompt, task])).length;
  return {
    schemaVersion: 2,
    id: 'lease:1',
    singleUse: true,
    personaId: descriptor.personaId,
    inputSha256: 'a'.repeat(64),
    inputBytes,
    systemPrompt: context.systemPrompt,
    task,
    corpusSnapshotId: descriptor.corpusSnapshotId,
    manifestId: null,
    purpose: 'research',
    formatAttempt: 1,
    policyVersion: descriptor.policyVersion,
    receiptIds: context.receiptIds,
    items: [],
    sources: [],
    contextSha256: 'b'.repeat(64),
    expiresAt: descriptor.expiresAt,
    usage: { modelCalls: 1, inputBytes },
  };
}
const draft = {
  schemaVersion: 1 as const,
  extractionVersion: 'extract:1',
  items: [],
  analyzedRefs: [],
  contextLeaseIds: ['lease:1'],
};

it('requests a bounded catalog on the open session and rejects foreign metadata', async () => {
  const port = await openNativeResearch(request);
  const catalog = {
    schemaVersion: 1,
    snapshotId: descriptor.corpusSnapshotId,
    indexGeneration: descriptor.indexGeneration,
    personaId: descriptor.personaId,
    policyVersion: descriptor.policyVersion,
    eligibleEpisodes: 1,
    samples: [{ episodeId: 'episode:1', sourceKey: 'binding:1', periodKey: 'undated' }],
  };
  host.invoke.mockResolvedValueOnce(catalog);
  expect(await port.getResearchCatalog(10)).toEqual(catalog);
  expect(host.invoke).toHaveBeenLastCalledWith('research_memory_catalog', {
    sessionId: descriptor.id,
    limit: 10,
  });
  const calls = host.invoke.mock.calls.length;
  await expect(port.getResearchCatalog(0)).rejects.toThrow();
  expect(host.invoke).toHaveBeenCalledTimes(calls);
  host.invoke.mockResolvedValueOnce({ ...catalog, personaId: 'foreign' });
  await expect(port.getResearchCatalog(10)).rejects.toThrow();
  expect(host.invoke).toHaveBeenLastCalledWith('close_memory_session', {
    sessionId: descriptor.id,
  });
});
function publication() {
  const publishedAt = '2026-11-09T03:00:00.000Z';
  return {
    schemaVersion: 1,
    id: 'publication:1',
    snapshot: {
      schemaVersion: 1,
      id: 'memory:1',
      corpusSnapshotId: request.snapshotId,
      itemIds: [],
      extractionVersion: draft.extractionVersion,
      createdAt: publishedAt,
    },
    itemIds: [],
    analyzedSpans: [],
    publishedAt,
    coverage: {
      loadedDocuments: 1,
      indexedDocuments: 1,
      analyzedDocuments: 0,
      usedDocuments: 0,
      totalDocuments: null,
      loadedBytes: 10,
      complete: false,
      limitations: [],
    },
  };
}
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<T>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}
beforeEach(() => {
  vi.resetAllMocks();
  host.desktop = true;
  host.invoke.mockImplementation((command) => {
    if (command === 'open_research_session') return Promise.resolve(structuredClone(descriptor));
    if (command === 'recall_memory') return Promise.resolve(retrieval());
    if (command === 'read_memory_episode') return Promise.resolve(retrieval('readEpisode'));
    if (command === 'reserve_memory_context') return Promise.resolve(lease());
    if (command === 'publish_memory_research') return Promise.resolve(publication());
    if (command === 'close_memory_session') return Promise.resolve(true);
    return Promise.resolve(null);
  });
});
afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
});
it('opens a native session and carries exact scoped commands through retrieval, context and publication', async () => {
  const port = await openNativeResearch(request);
  expect(port.session).toEqual(descriptor);
  const found = await port.recall('  Точный запрос 👩🏽‍🔬\r\n', 'facts');
  expect(found).toEqual({ ...retrieval(), items: [], limitations: [] });
  expect(await port.reserveModelContext(context)).toEqual(lease());
  expect(await port.publishResearch(draft)).toEqual(publication());
  await port.close();
  expect(host.invoke.mock.calls).toEqual([
    ['open_research_session', { request }],
    ['recall_memory', { sessionId: descriptor.id, queryText: '  Точный запрос 👩🏽‍🔬\r\n' }],
    ['reserve_memory_context', { sessionId: descriptor.id, request: context }],
    ['publish_memory_research', { sessionId: descriptor.id, draft }],
    ['close_memory_session', { sessionId: descriptor.id }],
  ]);
});
it('reads an episode through its own native command and retains the requested ID', async () => {
  const port = await openNativeResearch(request);
  expect(await port.readEpisode('episode:1')).toEqual({
    ...retrieval('readEpisode'),
    episodeId: 'episode:1',
  });
  expect(host.invoke).toHaveBeenLastCalledWith('read_memory_episode', {
    sessionId: descriptor.id,
    episodeId: 'episode:1',
  });
  await port.close();
});
it('rejects web execution, invalid admission and pre-cancellation without IPC', async () => {
  host.desktop = false;
  await expect(openNativeResearch(request)).rejects.toThrow('настольном');
  host.desktop = true;
  await expect(openNativeResearch({ ...request, jobId: '' })).rejects.toThrow();
  await expect(
    openNativeResearch(request, AbortSignal.abort(new Error('already stopped'))),
  ).rejects.toThrow('already stopped');
  expect(host.invoke).not.toHaveBeenCalled();
});
it('rejects invalid actions, foreign receipts and persona context before IPC', async () => {
  const port = await openNativeResearch(request);
  for (const query of ['', 'я'.repeat(2049), '\ud800'])
    await expect(port.recall(query, 'facts')).rejects.toThrow();
  await expect(port.readEpisode('')).rejects.toThrow();
  await expect(port.reserveModelContext(context)).rejects.toThrow();
  await expect(
    port.reserveModelContext({
      ...context,
      receiptIds: [],
      purpose: 'personaReply',
      manifestId: 'manifest:1',
    }),
  ).rejects.toThrow();
  await expect(port.publishResearch(draft)).rejects.toThrow();
  expect(host.invoke).toHaveBeenCalledTimes(1);
  await port.close();
});
it('keeps received receipt identity private from later caller mutation', async () => {
  const port = await openNativeResearch(request);
  const found = await port.recall('query', 'context');
  Object.assign(found.receipt, { id: 'forged' });
  expect(await port.reserveModelContext(context)).toEqual(lease());
  await expect(port.reserveModelContext({ ...context, receiptIds: ['forged'] })).rejects.toThrow();
  await port.close();
});
it('rejects overlapping operations without queueing or widening the session', async () => {
  const port = await openNativeResearch(request);
  const worker = deferred<unknown>();
  host.invoke.mockImplementationOnce(() => worker.promise);
  const first = port.recall('first', 'style');
  await expect(port.recall('second', 'facts')).rejects.toThrow('занята');
  expect(host.invoke).toHaveBeenCalledTimes(2);
  worker.resolve(retrieval());
  await first;
  await port.close();
});
it('closes a malformed native admission instead of leaving an undisclosed handle', async () => {
  host.invoke.mockResolvedValueOnce({ ...descriptor, personaId: 'foreign' });
  await expect(openNativeResearch(request)).rejects.toThrow();
  expect(host.invoke).toHaveBeenLastCalledWith('close_memory_session', {
    sessionId: descriptor.id,
  });
});
it('retries early cancellation until the worker ends and closes a late successful admission', async () => {
  vi.useFakeTimers();
  const worker = deferred<unknown>();
  host.invoke.mockImplementationOnce(() => worker.promise);
  const controller = new AbortController();
  const result = openNativeResearch(request, controller.signal);
  const outcome = result.then(
    (value) => value,
    (error: unknown) => error,
  );
  controller.abort(new Error('stop opening'));
  await vi.advanceTimersByTimeAsync(250);
  expect(
    host.invoke.mock.calls.filter(([name]) => name === 'cancel_ingestion_operation').length,
  ).toBeGreaterThan(1);
  worker.resolve(descriptor);
  expect(await outcome).toEqual(new Error('stop opening'));
  expect(host.invoke).toHaveBeenCalledWith('close_memory_session', { sessionId: descriptor.id });
  expect(vi.getTimerCount()).toBe(0);
});
it('cancels an active read, waits for its native worker and never exposes the late result', async () => {
  const port = await openNativeResearch(request);
  const worker = deferred<unknown>();
  host.invoke.mockImplementationOnce(() => worker.promise);
  const controller = new AbortController();
  const result = port.recall('query', 'facts', controller.signal);
  const rejected = expect(result).rejects.toThrow('stop read');
  controller.abort(new Error('stop read'));
  await vi.waitFor(() =>
    expect(host.invoke).toHaveBeenCalledWith('close_memory_session', { sessionId: descriptor.id }),
  );
  worker.resolve(retrieval());
  await rejected;
  await expect(port.readEpisode('episode:1')).rejects.toThrow('закрыта');
  await port.close();
});
it('close is idempotent, waits for an active worker and forbids further operations', async () => {
  const port = await openNativeResearch(request);
  const worker = deferred<unknown>();
  host.invoke.mockImplementationOnce(() => worker.promise);
  const running = port.recall('query', 'facts');
  const rejected = expect(running).rejects.toThrow('закрыта');
  let closed = false;
  const closing = port.close().then(() => {
    closed = true;
  });
  await Promise.resolve();
  expect(closed).toBe(false);
  worker.resolve(retrieval());
  await rejected;
  await closing;
  await port.close();
  await expect(port.recall('again', 'facts')).rejects.toThrow('закрыта');
  expect(host.invoke.mock.calls.filter(([name]) => name === 'close_memory_session')).toHaveLength(
    1,
  );
});
it('rejects malformed retrieval responses and invalidates the session', async () => {
  const port = await openNativeResearch(request);
  host.invoke.mockResolvedValueOnce({ ...retrieval(), sources: [{}] });
  await expect(port.recall('query', 'facts')).rejects.toThrow();
  expect(host.invoke).toHaveBeenLastCalledWith('close_memory_session', {
    sessionId: descriptor.id,
  });
  await expect(port.recall('again', 'facts')).rejects.toThrow('закрыта');
});
it('can retry publication after a lost response without claiming a fabricated result', async () => {
  const port = await openNativeResearch(request);
  await port.recall('query', 'facts');
  await port.reserveModelContext(context);
  host.invoke.mockRejectedValueOnce(new Error('transport failed'));
  await expect(port.publishResearch(draft)).rejects.toThrow('transport failed');
  expect(await port.publishResearch(draft)).toEqual(publication());
  await port.close();
});

it('rejects episode IDs with invalid Unicode, controls or excessive UTF8 bytes before IPC', async () => {
  const port = await openNativeResearch(request);
  for (const id of ['\ud800', 'episode\n1', 'я'.repeat(129)])
    await expect(port.readEpisode(id)).rejects.toThrow();
  expect(host.invoke).toHaveBeenCalledTimes(1);
  await port.close();
});
it('rejects cancellation arriving after decoding but before the operation returns', async () => {
  const port = await openNativeResearch(request);
  const worker = deferred<unknown>();
  host.invoke.mockImplementationOnce(() => worker.promise);
  const controller = new AbortController();
  const result = port.recall('query', 'facts', controller.signal);
  const outcome = result.then(
    (value) => value,
    (error: unknown) => error,
  );
  worker.resolve(retrieval());
  queueMicrotask(() => controller.abort(new Error('late stop')));
  expect(await outcome).toEqual(new Error('late stop'));
  await port.close();
});

it('carries original source wording and authorship into the exact native context without retaining caller mutations', async () => {
  const port = await openNativeResearch(request);
  const { text, ...document } = sourceFixture.sourceResult.documents[0]!;
  const span = {
    documentId: document.id,
    revisionId: 'revision:1',
    unit: 'utf8Bytes',
    start: 0,
    end: new TextEncoder().encode(text).length,
  };
  const source = {
    document: { ...document, partition: 'reference' },
    span,
    text,
    role: 'context',
    speakerRelation: 'unknown',
    canSupportOwnWords: false,
    relations: [],
  };
  const selected = {
    receipt: {
      ...retrieval().receipt,
      sourceBytes: span.end,
      contextTokens: 20,
      hits: [
        {
          chunkId: 'chunk:1',
          episodeId: 'episode:1',
          spans: [span],
          lexicalRank: 1,
          semanticRank: null,
          fusionScore: 1 / 61,
        },
      ],
    },
    sources: [source],
  };
  host.invoke.mockResolvedValueOnce(selected);
  const result = await port.recall('query', 'facts');
  expect(result.sources[0]?.text).toBe(text);
  expect(result.sources[0]?.document.authorLabel).toBe(document.authorLabel);
  Object.assign(result.sources[0]!.document, { authorLabel: 'Caller mutation' });
  const task = context.task + '\n' + JSON.stringify(selected);
  const inputBytes = new TextEncoder().encode(JSON.stringify([context.systemPrompt, task])).length;
  const native = {
    ...lease(),
    task,
    inputBytes,
    sources: [source],
    usage: { modelCalls: 1, inputBytes },
  };
  host.invoke.mockResolvedValueOnce(native);
  const reserved = await port.reserveModelContext(context);
  expect(reserved).toEqual(native);
  expect(reserved.sources[0]?.document.authorLabel).toBe(document.authorLabel);
  await port.close();
});
it('reports a failed close and permits retry without reopening the session', async () => {
  const port = await openNativeResearch(request);
  host.invoke.mockRejectedValueOnce(new Error('close transport failure'));
  await expect(port.close()).rejects.toThrow('close transport failure');
  await expect(port.recall('query', 'facts')).rejects.toThrow('закрыта');
  host.invoke.mockResolvedValueOnce(false);
  await port.close();
  await port.close();
  expect(host.invoke.mock.calls.filter(([name]) => name === 'close_memory_session')).toHaveLength(
    2,
  );
});
