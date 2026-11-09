import { beforeEach, expect, it, vi } from 'vitest';
import { decodeSourceExcerpt } from '@proteus-int/contracts';
import { nativeCorpusInspection } from './corpus-inspection';
import sourceFixture from './fixtures/native-wire.json';
const native = vi.hoisted(() => ({ desktop: true, invoke: vi.fn() }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: native.invoke }));
vi.mock('./window-controls', () => ({ isDesktop: () => native.desktop }));
const operationId = '00000000-0000-4000-8000-000000000001';
const query = {
  jobId: 'job',
  snapshotId: 'snapshot',
  personaId: 'persona',
  episodeId: 'episode',
  period: { from: null, to: null, includeUndated: true },
  expectedIndexGeneration: 2,
  maxDurationMs: 30000,
};
function result() {
  const { text, ...document } = sourceFixture.sourceResult.documents[0]!;
  const excerpt = decodeSourceExcerpt({
    document: { ...document, partition: 'reference' },
    text,
    span: {
      documentId: document.id,
      revisionId: 'revision',
      unit: 'utf8Bytes',
      start: 0,
      end: new TextEncoder().encode(text).length,
    },
    role: 'reference',
    speakerRelation: 'unknown',
    canSupportOwnWords: false,
    relations: [],
    sourceMetadata: {
      schemaVersion: 1,
      sourceFormat: 'synthetic',
      resourceLabel: null,
      title: null,
      authorLabels: [],
      originalObjectId: null,
      limitations: [],
    },
  });
  return {
    schemaVersion: 1,
    snapshotId: query.snapshotId,
    indexGeneration: 2,
    personaId: query.personaId,
    episodeId: query.episodeId,
    sourceBytes: new TextEncoder().encode(text).length,
    contextTokens: 3,
    chunks: [{ chunkId: 'chunk', excerpt }],
  };
}
beforeEach(() => {
  vi.restoreAllMocks();
  vi.resetAllMocks();
  native.desktop = true;
  vi.spyOn(crypto, 'randomUUID').mockReturnValue(operationId);
});
it('reads the pinned episode through native inspection and retains exact returned source text', async () => {
  const value = result();
  native.invoke.mockResolvedValue(value);
  expect(await nativeCorpusInspection.inspectEpisode(query)).toEqual(value);
  expect(native.invoke).toHaveBeenCalledExactlyOnceWith('inspect_ingestion_episode', {
    request: { schemaVersion: 1, operationId, ...query },
  });
});
it('rejects scope and generation substitution without returning source text', async () => {
  for (const field of ['snapshotId', 'personaId', 'episodeId', 'indexGeneration']) {
    native.invoke.mockResolvedValue({
      ...result(),
      [field]: field === 'indexGeneration' ? 3 : 'foreign',
    });
    await expect(nativeCorpusInspection.inspectEpisode(query)).rejects.toThrow();
  }
});
it('never invokes native inspection in browser mode or after prior cancellation', async () => {
  native.desktop = false;
  await expect(nativeCorpusInspection.inspectEpisode(query)).rejects.toThrow();
  native.desktop = true;
  const controller = new AbortController();
  const failure = new Error('cancelled by caller');
  controller.abort(failure);
  await expect(nativeCorpusInspection.inspectEpisode(query, controller.signal)).rejects.toBe(
    failure,
  );
  expect(native.invoke).not.toHaveBeenCalled();
});

it('discards a late source response after cancellation and confirms the native stop', async () => {
  const controller = new AbortController();
  let finish!: (value: unknown) => void;
  native.invoke.mockImplementation((command: string) =>
    command === 'inspect_ingestion_episode'
      ? new Promise((resolve) => {
          finish = resolve;
        })
      : Promise.resolve(),
  );
  const pending = nativeCorpusInspection.inspectEpisode(query, controller.signal);
  controller.abort(new Error('view closed'));
  const rejected = expect(pending).rejects.toThrow('view closed');
  finish(result());
  await rejected;
  expect(
    native.invoke.mock.calls.filter(([command]) => command === 'cancel_ingestion_operation'),
  ).toHaveLength(2);
  expect(native.invoke).toHaveBeenLastCalledWith('cancel_ingestion_operation', { operationId });
});
it('pins caller scope before waiting for native and reports revoked access unchanged', async () => {
  const selected = structuredClone(query);
  const value = result();
  native.invoke.mockImplementation(() => {
    selected.personaId = 'changed-after-call';
    return Promise.resolve(value);
  });
  expect(await nativeCorpusInspection.inspectEpisode(selected)).toEqual(value);
  native.invoke.mockRejectedValueOnce(new Error('Access revoked'));
  await expect(nativeCorpusInspection.inspectEpisode(query)).rejects.toThrow('Access revoked');
});
