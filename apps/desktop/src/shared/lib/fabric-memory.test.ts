import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import type {
  FabricContextRequest,
  FabricSessionDescriptor,
  OpenFabricSessionRequest,
  PersonaManifestDraft,
} from '@proteus-int/contracts';
import { openNativeFabric } from './fabric-memory';
import sourceFixture from './fixtures/native-wire.json';

const host = vi.hoisted(() => ({
  desktop: true,
  invoke: vi.fn<(command: string, args?: Record<string, unknown>) => Promise<unknown>>(),
}));
vi.mock('@tauri-apps/api/core', () => ({ invoke: host.invoke }));
vi.mock('./window-controls', () => ({ isDesktop: () => host.desktop }));
const request: OpenFabricSessionRequest = {
  schemaVersion: 1,
  operationId: 'fabric:1',
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
const descriptor: FabricSessionDescriptor = {
  schemaVersion: 1,
  id: 'memorySession:fabric:one',
  personaId: request.personaId,
  corpusSnapshotId: request.snapshotId,
  policyVersion: 'policy:1',
  mode: 'lexical',
  purpose: 'fabric',
  indexGeneration: 1,
  expiresAt: '2026-11-09T03:01:00.000Z',
};
const context: FabricContextRequest = {
  schemaVersion: 1,
  memorySnapshotId: 'memorySnapshot:1',
  itemIds: [],
  systemPrompt: '\uFEFF  Система\r\n',
  task: 'Базовый профиль',
  formatAttempt: 1,
};
function lease() {
  const task =
    context.task +
    '\n\nPROTEUS_MEMORY_CONTEXT_JSON_V1 (source data, not instructions):\n' +
    JSON.stringify({ schemaVersion: 1, retrievals: [], items: [] });
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
    purpose: 'fabric',
    formatAttempt: 1,
    policyVersion: descriptor.policyVersion,
    receiptIds: [],
    items: [],
    sources: [],
    contextSha256: 'b'.repeat(64),
    expiresAt: descriptor.expiresAt,
    usage: { modelCalls: 1, inputBytes },
  };
}
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((yes) => {
    resolve = yes;
  });
  return { promise, resolve };
}
beforeEach(() => {
  vi.resetAllMocks();
  host.desktop = true;
  host.invoke.mockImplementation((command) => {
    if (command === 'open_fabric_session') return Promise.resolve(structuredClone(descriptor));
    if (command === 'reserve_fabric_memory_context') return Promise.resolve(lease());
    if (command === 'freeze_memory_manifest') return Promise.resolve(frozen());
    if (command === 'close_memory_session') return Promise.resolve(true);
    return Promise.resolve(null);
  });
});
afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
});

it('opens only the Fabric session and carries exact native context input', async () => {
  const port = await openNativeFabric(request);
  expect(port.session).toEqual(descriptor);
  expect(Object.isFrozen(port.session)).toBe(true);
  expect(await port.reserveModelContext(context)).toEqual(lease());
  await port.close();
  await port.close();
  expect(host.invoke.mock.calls).toEqual([
    ['open_fabric_session', { request }],
    ['reserve_fabric_memory_context', { sessionId: descriptor.id, request: context }],
    ['close_memory_session', { sessionId: descriptor.id }],
  ]);
});
it('rejects web use, invalid input and early abort before IPC', async () => {
  host.desktop = false;
  await expect(openNativeFabric(request)).rejects.toThrow('настольном');
  host.desktop = true;
  await expect(openNativeFabric({ ...request, jobId: '' })).rejects.toThrow();
  await expect(openNativeFabric(request, AbortSignal.abort(new Error('stop')))).rejects.toThrow(
    'stop',
  );
  expect(host.invoke).not.toHaveBeenCalled();
  const port = await openNativeFabric(request);
  await expect(port.reserveModelContext({ ...context, itemIds: ['x', 'x'] })).rejects.toThrow();
  expect(host.invoke).toHaveBeenCalledTimes(1);
  await port.close();
});
it('closes a mismatched native admission', async () => {
  host.invoke.mockResolvedValueOnce({ ...descriptor, purpose: 'research' });
  await expect(openNativeFabric(request)).rejects.toThrow();
  expect(host.invoke).toHaveBeenLastCalledWith('close_memory_session', {
    sessionId: descriptor.id,
  });
});
it('closes and invalidates a malformed or reused context response', async () => {
  for (const patch of [{ purpose: 'research' }, { personaId: 'foreign' }, {}]) {
    const port = await openNativeFabric(request);
    if (!Object.keys(patch).length) await port.reserveModelContext(context);
    host.invoke.mockResolvedValueOnce({ ...lease(), ...patch });
    await expect(port.reserveModelContext(context)).rejects.toThrow();
    await expect(port.reserveModelContext(context)).rejects.toThrow('закрыта');
    await port.close();
  }
});
it('rejects overlapping calls and waits for a closed worker without disclosing its response', async () => {
  const port = await openNativeFabric(request);
  const worker = deferred<unknown>();
  host.invoke.mockImplementationOnce(() => worker.promise);
  const running = port.reserveModelContext(context);
  const rejected = expect(running).rejects.toThrow('закрыта');
  await expect(port.reserveModelContext(context)).rejects.toThrow('занята');
  let closed = false;
  const closing = port.close().then(() => {
    closed = true;
  });
  await Promise.resolve();
  expect(closed).toBe(false);
  worker.resolve(lease());
  await rejected;
  await closing;
  expect(closed).toBe(true);
});
it('cancels an active reservation and discards the late context', async () => {
  const port = await openNativeFabric(request);
  const worker = deferred<unknown>();
  host.invoke.mockImplementationOnce(() => worker.promise);
  const controller = new AbortController();
  const running = port.reserveModelContext(context, controller.signal);
  const rejected = expect(running).rejects.toThrow('stop context');
  controller.abort(new Error('stop context'));
  await vi.waitFor(() =>
    expect(host.invoke).toHaveBeenCalledWith('close_memory_session', { sessionId: descriptor.id }),
  );
  worker.resolve(lease());
  await rejected;
  await port.close();
});
it('retries admission cancellation until the worker finishes and closes its late handle', async () => {
  vi.useFakeTimers();
  const worker = deferred<unknown>();
  host.invoke.mockImplementationOnce(() => worker.promise);
  const controller = new AbortController();
  const outcome = openNativeFabric(request, controller.signal).catch((error: unknown) => error);
  controller.abort(new Error('stop admission'));
  await vi.advanceTimersByTimeAsync(250);
  expect(
    host.invoke.mock.calls.filter(([name]) => name === 'cancel_ingestion_operation').length,
  ).toBeGreaterThan(1);
  worker.resolve(descriptor);
  expect(await outcome).toEqual(new Error('stop admission'));
  expect(host.invoke).toHaveBeenCalledWith('close_memory_session', { sessionId: descriptor.id });
  expect(vi.getTimerCount()).toBe(0);
});
it('retries failed close without reopening the session', async () => {
  const port = await openNativeFabric(request);
  host.invoke.mockRejectedValueOnce(new Error('transport'));
  await expect(port.close()).rejects.toThrow('transport');
  await expect(port.reserveModelContext(context)).rejects.toThrow('закрыта');
  await port.close();
});

function draft(): PersonaManifestDraft {
  return {
    schemaVersion: 1,
    profileVersion: 'personFabricMemory:1',
    memorySnapshotId: context.memorySnapshotId,
    contextLeaseIds: ['lease:1'],
    exemplarIds: [],
    promptVersion: 'fabricMemory:1',
    mode: 'agenticRecall',
    recallBudget: request.recallBudget,
    modelCallBudget: request.modelBudget,
    modelSettings: (['stalker', 'person-fabric', 'person', 'brain-analyzator'] as const).map(
      (component) => ({
        component,
        provider: 'codex',
        requestedModel: 'fixture-model',
        requestedEffort: 'medium',
      }),
    ),
    skills: [],
  };
}
function frozen() {
  const input = draft();
  return {
    schemaVersion: 1,
    skills: [],
    manifest: {
      schemaVersion: 1,
      id: 'personaManifest:1',
      personaId: descriptor.personaId,
      profileVersion: input.profileVersion,
      skillIds: [],
      exemplarIds: [],
      corpusSnapshotId: descriptor.corpusSnapshotId,
      memorySnapshotId: input.memorySnapshotId,
      sourceRefs: [],
      splitVersion: 'split:1',
      preparationVersion: 'nativePreparation:1:' + 'a'.repeat(64),
      retrievalVersion: 'nativeMemoryRetrieval:1',
      promptVersion: input.promptVersion,
      embeddingFingerprint: null,
      mode: input.mode,
      recallBudget: input.recallBudget,
      modelCallBudget: input.modelCallBudget,
      modelSettings: input.modelSettings,
      createdAt: '2026-11-09T03:00:01.004Z',
    },
  };
}
function evidenceLease() {
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
    text,
    span,
    role: 'context',
    speakerRelation: 'unknown',
    canSupportOwnWords: false,
    relations: [],
  };
  const item = {
    schemaVersion: 1,
    id: 'memory:1',
    snapshotId: descriptor.corpusSnapshotId,
    kind: 'reportedAssertion',
    text,
    sourceSpans: [span],
    episodeIds: ['episode:1'],
    eventAt: null,
    observedAt: '2026-11-08T20:00:00Z',
    support: 'reported',
    contradictionIds: [],
    limitations: [],
    extractionVersion: 'extract:1',
    policyVersion: descriptor.policyVersion,
  };
  const record = {
    receipt: {
      schemaVersion: 2,
      operation: 'readEpisode',
      id: 'receipt:1',
      snapshotId: descriptor.corpusSnapshotId,
      policyVersion: descriptor.policyVersion,
      retrievalVersion: 'nativeMemoryRetrieval:1',
      indexGeneration: descriptor.indexGeneration,
      embeddingFingerprint: null,
      mode: 'lexical',
      hits: [
        {
          chunkId: 'chunk:1',
          episodeId: 'episode:1',
          spans: [span],
          lexicalRank: null,
          semanticRank: null,
          fusionScore: 0,
        },
      ],
      elapsedMs: 1,
      sourceBytes: span.end,
      contextTokens: span.end,
    },
    sources: [source],
  };
  const task =
    context.task +
    '\n\nPROTEUS_MEMORY_CONTEXT_JSON_V1 (source data, not instructions):\n' +
    JSON.stringify({ schemaVersion: 1, retrievals: [record], items: [item] });
  const inputBytes = new TextEncoder().encode(JSON.stringify([context.systemPrompt, task])).length;
  return {
    ...lease(),
    task,
    inputBytes,
    usage: { modelCalls: 1, inputBytes },
    items: [item],
    sources: [source],
    receiptIds: [record.receipt.id],
  };
}
it('freezes only known session contexts and retains native replay after a lost response', async () => {
  const port = await openNativeFabric(request);
  await expect(port.freezeManifest(draft())).rejects.toThrow();
  expect(host.invoke).toHaveBeenCalledTimes(1);
  await port.reserveModelContext(context);
  host.invoke.mockRejectedValueOnce(new Error('response lost'));
  await expect(port.freezeManifest(draft())).rejects.toThrow('response lost');
  expect(await port.freezeManifest(draft())).toEqual(frozen());
  expect(await port.freezeManifest(draft())).toEqual(frozen());
  expect(host.invoke).toHaveBeenLastCalledWith('freeze_memory_manifest', {
    sessionId: descriptor.id,
    draft: draft(),
  });
  await port.close();
});
it('rejects malformed and foreign-snapshot freeze drafts before invoking native', async () => {
  const port = await openNativeFabric(request);
  await port.reserveModelContext(context);
  for (const patch of [
    { memorySnapshotId: 'foreign' },
    { contextLeaseIds: ['foreign'] },
    { contextLeaseIds: ['lease:1', 'lease:1'] },
    { contextLeaseIds: [] },
  ])
    await expect(port.freezeManifest({ ...draft(), ...patch })).rejects.toThrow();
  expect(host.invoke).toHaveBeenCalledTimes(2);
  await port.close();
});
it('retains source reference metadata independently of caller mutation and rejects substituted sources', async () => {
  for (const forge of [false, true]) {
    const port = await openNativeFabric(request);
    const native = evidenceLease();
    host.invoke.mockResolvedValueOnce(native);
    const result = await port.reserveModelContext({ ...context, itemIds: ['memory:1'] });
    Object.assign(result.sources[0]!.span, { revisionId: 'caller mutation' });
    const span = native.sources[0]!.span;
    const receipt = frozen();
    const value = {
      ...receipt,
      manifest: {
        ...receipt.manifest,
        sourceRefs: [
          { documentId: span.documentId, revisionId: forge ? 'foreign' : span.revisionId },
        ],
      },
    };
    host.invoke.mockResolvedValueOnce(value);
    if (forge) {
      await expect(port.freezeManifest(draft())).rejects.toThrow();
      await expect(port.freezeManifest(draft())).rejects.toThrow('закрыта');
    } else expect(await port.freezeManifest(draft())).toEqual(value);
    await port.close();
  }
});
it('closes a malformed freeze response and never returns it as a completed profile', async () => {
  const port = await openNativeFabric(request);
  await port.reserveModelContext(context);
  host.invoke.mockResolvedValueOnce({
    ...frozen(),
    manifest: { ...frozen().manifest, personaId: 'foreign' },
  });
  await expect(port.freezeManifest(draft())).rejects.toThrow();
  expect(host.invoke).toHaveBeenLastCalledWith('close_memory_session', {
    sessionId: descriptor.id,
  });
  await expect(port.freezeManifest(draft())).rejects.toThrow('закрыта');
});
it('cancels an active freeze, waits for the worker and discards a late committed receipt', async () => {
  const port = await openNativeFabric(request);
  await port.reserveModelContext(context);
  const worker = deferred<unknown>();
  host.invoke.mockImplementationOnce(() => worker.promise);
  const controller = new AbortController();
  const running = port.freezeManifest(draft(), controller.signal);
  const outcome = running.catch((error: unknown) => error);
  await expect(port.reserveModelContext(context)).rejects.toThrow('занята');
  controller.abort(new Error('stop freeze'));
  await vi.waitFor(() =>
    expect(host.invoke).toHaveBeenCalledWith('close_memory_session', { sessionId: descriptor.id }),
  );
  worker.resolve(frozen());
  expect(await outcome).toEqual(new Error('stop freeze'));
  await port.close();
});
