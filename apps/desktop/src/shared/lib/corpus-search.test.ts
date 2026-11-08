import { beforeEach, expect, it, vi } from 'vitest';
import { nativeCorpusSearch } from './corpus-search';
const native = vi.hoisted(() => ({ desktop: true, invoke: vi.fn() }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: native.invoke }));
vi.mock('./window-controls', () => ({ isDesktop: () => native.desktop }));
const operationId = '00000000-0000-4000-8000-000000000001';
const query = {
  jobId: 'job',
  snapshotId: 'snapshot',
  personaId: 'personA',
  queryText: '  контекст é\r\n',
  period: { from: null, to: null, includeUndated: false },
  limit: 10,
  offset: 0,
  expectedIndexGeneration: null,
  maxDurationMs: 60000,
};
const result = {
  schemaVersion: 1,
  snapshotId: 'snapshot',
  snapshotGeneration: 1,
  indexGeneration: 2,
  hasMore: false,
  hits: [
    {
      chunkId: 'chunk',
      snapshotId: 'snapshot',
      indexGeneration: 2,
      bm25: -0.5,
      documentId: 'document',
      revisionId: 'revision',
      spanStart: 0,
      spanEnd: 8,
      personaId: 'personA',
      grantId: 'grant',
      bindingId: 'binding',
      periodFrom: null,
      periodTo: null,
      conversationId: null,
      threadId: null,
    },
  ],
};
const receipt = {
  schemaVersion: 1,
  snapshotId: 'snapshot',
  snapshotGeneration: 1,
  indexGeneration: 2,
  policyFingerprint: 'nativeFts:1:policy',
  contentSha256: 'a'.repeat(64),
  chunkCount: 1,
  createdAt: '1794140000',
};
beforeEach(() => {
  vi.restoreAllMocks();
  vi.resetAllMocks();
  native.desktop = true;
  vi.spyOn(crypto, 'randomUUID').mockReturnValue(operationId);
});
it('queries exact text with host-owned scope through the versioned native command', async () => {
  native.invoke.mockResolvedValue(result);
  expect(await nativeCorpusSearch.search(query)).toEqual(result);
  expect(native.invoke).toHaveBeenCalledExactlyOnceWith('search_ingestion_corpus', {
    request: { schemaVersion: 1, operationId, ...query },
  });
});
it('rebuilds only the requested native snapshot and returns its index receipt', async () => {
  native.invoke.mockResolvedValue(receipt);
  expect(await nativeCorpusSearch.rebuild('job', 'snapshot', 3600000)).toEqual(receipt);
  expect(native.invoke).toHaveBeenCalledExactlyOnceWith('rebuild_ingestion_index', {
    request: {
      schemaVersion: 1,
      operationId,
      jobId: 'job',
      snapshotId: 'snapshot',
      maxDurationMs: 3600000,
    },
  });
  native.invoke.mockResolvedValue({ ...receipt, snapshotId: 'foreign' });
  await expect(nativeCorpusSearch.rebuild('job', 'snapshot', 1000)).rejects.toThrow();
});
it('rejects responses from another persona or snapshot, changed pins, and excess hits', async () => {
  for (const invalid of [
    { ...result, snapshotId: 'foreign', hits: [] },
    { ...result, hits: [{ ...result.hits[0], personaId: 'foreign' }] },
    { ...result, hits: [...result.hits, { ...result.hits[0], chunkId: 'another' }] },
  ]) {
    native.invoke.mockResolvedValue(invalid);
    await expect(nativeCorpusSearch.search({ ...query, limit: 1 })).rejects.toThrow();
  }
  native.invoke.mockResolvedValue(result);
  await expect(
    nativeCorpusSearch.search({ ...query, expectedIndexGeneration: 3 }),
  ).rejects.toThrow();
});
it('validates query and rebuild budgets before invoking native', async () => {
  for (const invalid of [
    { ...query, limit: 0 },
    { ...query, grantIds: ['injected'] },
    { ...query, offset: 1 },
  ]) {
    await expect(nativeCorpusSearch.search(invalid)).rejects.toThrow();
  }
  for (const budget of [0, 3600001, NaN, 1.5]) {
    await expect(nativeCorpusSearch.rebuild('job', 'snapshot', budget)).rejects.toThrow();
  }
  await expect(nativeCorpusSearch.rebuild('я'.repeat(65), 'snapshot', 1)).rejects.toThrow();
  expect(native.invoke).not.toHaveBeenCalled();
});
it('fails explicitly outside desktop and never invokes a fabricated search', async () => {
  native.desktop = false;
  await expect(nativeCorpusSearch.search(query)).rejects.toThrow('настольном');
  expect(native.invoke).not.toHaveBeenCalled();
});
it('discards a cancelled late search and cancels the operation without cancelling the import', async () => {
  let resolve!: (value: unknown) => void;
  const pending = new Promise<unknown>((done) => {
    resolve = done;
  });
  native.invoke.mockImplementation((command: string) =>
    command === 'search_ingestion_corpus' ? pending : Promise.resolve(undefined),
  );
  const controller = new AbortController();
  const response = nativeCorpusSearch.search(query, controller.signal);
  const rejected = expect(response).rejects.toThrow('stop search');
  controller.abort(new Error('stop search'));
  resolve(result);
  await rejected;
  expect(native.invoke).toHaveBeenCalledWith('cancel_ingestion_operation', { operationId });
  expect(native.invoke.mock.calls.some(([command]) => command === 'cancel_ingestion_job')).toBe(
    false,
  );
});
it('never starts a pre-cancelled operation', async () => {
  const controller = new AbortController();
  controller.abort(new Error('already cancelled'));
  await expect(nativeCorpusSearch.search(query, controller.signal)).rejects.toThrow(
    'already cancelled',
  );
  expect(native.invoke).not.toHaveBeenCalled();
});
