import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import type {
  OpenPersonaSessionRequest,
  PersonaSessionDescriptor,
  PersonaContextRequest,
  RecallResult,
} from '@proteus-int/contracts';
import { openNativePersona } from './persona-memory';
import { decodeSourceExcerpt } from '@proteus-int/contracts';
import sourceFixture from './fixtures/native-wire.json';

const host = vi.hoisted(() => ({
  desktop: true,
  invoke: vi.fn<(command: string, args?: Record<string, unknown>) => Promise<unknown>>(),
}));
vi.mock('@tauri-apps/api/core', () => ({ invoke: host.invoke }));
vi.mock('./window-controls', () => ({ isDesktop: () => host.desktop }));
const request: OpenPersonaSessionRequest = {
  schemaVersion: 1,
  operationId: 'person:1',
  jobId: 'job:1',
  personaId: 'persona:1',
  manifestId: 'manifest:1',
  period: { from: null, to: null, includeUndated: true },
};
function descriptor(): PersonaSessionDescriptor {
  return {
    schemaVersion: 1,
    id: 'memorySession:person:1',
    personaId: request.personaId,
    corpusSnapshotId: 'corpus:1',
    policyVersion: 'policy:1',
    mode: 'lexical',
    indexGeneration: 1,
    expiresAt: '2026-11-09T03:01:00.000Z',
    purpose: 'personaReply',
    manifest: {
      schemaVersion: 1,
      id: request.manifestId,
      personaId: request.personaId,
      profileVersion: 'profile:1',
      skillIds: [],
      exemplarIds: [],
      corpusSnapshotId: 'corpus:1',
      memorySnapshotId: 'memory:1',
      sourceRefs: [],
      splitVersion: 'split:1',
      preparationVersion: 'nativePreparation:1:abc',
      retrievalVersion: 'nativeMemoryRetrieval:1',
      promptVersion: 'prompt:1',
      embeddingFingerprint: null,
      mode: 'agenticRecall',
      recallBudget: {
        maxCalls: 4,
        maxCandidates: 10,
        maxEpisodes: 10,
        maxSourceBytes: 10000,
        maxContextTokens: 5000,
        maxDurationMs: 60000,
      },
      modelCallBudget: { maxCalls: 3, maxInputBytes: 10000, maxDurationMs: 60000 },
      modelSettings: (['stalker', 'person-fabric', 'person', 'brain-analyzator'] as const).map(
        (component) => ({
          component,
          provider: 'codex',
          requestedModel: 'fixture-model',
          requestedEffort: 'medium',
        }),
      ),
      createdAt: '2026-11-09T03:00:00Z',
    },
  };
}
const context: PersonaContextRequest = {
  schemaVersion: 2,
  purpose: 'personaReply',
  manifestId: request.manifestId,
  systemPrompt: '\uFEFFЯ отвечаю от первого лица\r\n',
  task: '  Задача é 😀\r\n',
  receiptIds: [],
  memoryItemIds: [],
  formatAttempt: 1,
};
type Retrieved = Pick<RecallResult, 'receipt' | 'sources'>;
function retrieval(id = 'receipt:1'): Retrieved {
  return {
    receipt: {
      schemaVersion: 2,
      operation: 'recall',
      id,
      snapshotId: 'corpus:1',
      policyVersion: 'policy:1',
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
function lease(records: readonly Retrieved[] = [], selected = context) {
  const task =
    selected.task +
    '\n\nPROTEUS_MEMORY_CONTEXT_JSON_V1 (source data, not instructions):\n' +
    JSON.stringify({ schemaVersion: 1, items: [], retrievals: records });
  const inputBytes = new TextEncoder().encode(JSON.stringify([selected.systemPrompt, task])).length;
  return {
    schemaVersion: 2,
    id: 'lease:1',
    singleUse: true,
    personaId: request.personaId,
    inputSha256: 'a'.repeat(64),
    inputBytes,
    systemPrompt: selected.systemPrompt,
    task,
    corpusSnapshotId: 'corpus:1',
    manifestId: request.manifestId,
    purpose: 'personaReply',
    formatAttempt: selected.formatAttempt,
    policyVersion: 'policy:1',
    receiptIds: records.map((r) => r.receipt.id),
    items: [],
    sources: records.flatMap((r) => r.sources),
    contextSha256: 'b'.repeat(64),
    expiresAt: descriptor().expiresAt,
    usage: { modelCalls: 1, inputBytes },
  };
}
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((yes) => {
    resolve = yes;
  });
  return { resolve, promise };
}
beforeEach(() => {
  vi.resetAllMocks();
  host.desktop = true;
  host.invoke.mockImplementation((command) => {
    if (command === 'open_persona_session') return Promise.resolve(descriptor());
    if (command === 'reserve_memory_context') return Promise.resolve(lease());
    if (command === 'recall_memory') return Promise.resolve(retrieval());
    if (command === 'close_memory_session') return Promise.resolve(true);
    return Promise.resolve(null);
  });
});
afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
});
it('opens only the native manifest scope, reserves exact Persona input and closes once', async () => {
  const port = await openNativePersona(request);
  expect(port.session).toEqual(descriptor());
  expect(await port.reserveModelContext(context)).toEqual(lease());
  await port.close();
  await port.close();
  expect(host.invoke.mock.calls).toEqual([
    ['open_persona_session', { request }],
    ['reserve_memory_context', { sessionId: descriptor().id, request: context }],
    ['close_memory_session', { sessionId: descriptor().id }],
  ]);
});
it('owns detached manifest settings and receipts despite consumer mutation', async () => {
  const port = await openNativePersona(request);
  const shown = port.session;
  expect(Object.isFrozen(shown)).toBe(true);
  Object.assign(shown.manifest.modelCallBudget, { maxCalls: 1000 });
  expect(port.session.manifest.modelCallBudget.maxCalls).toBe(3);
  const result = await port.recall('  Текст 😀\r\n', 'facts');
  Object.assign(result.receipt, { id: 'forged' });
  host.invoke.mockResolvedValueOnce(lease([retrieval()]));
  expect(await port.reserveModelContext({ ...context, receiptIds: ['receipt:1'] })).toEqual(
    lease([retrieval()]),
  );
  await port.close();
});
it('rejects foreign receipts, changed purpose and fixed-context actions before IPC', async () => {
  const base = descriptor();
  const session = { ...base, manifest: { ...base.manifest, mode: 'fixedContext' as const } };
  host.invoke.mockResolvedValueOnce(session);
  const port = await openNativePersona(request);
  const calls = host.invoke.mock.calls.length;
  await expect(port.recall('query', 'facts')).rejects.toThrow();
  await expect(port.readEpisode('episode:1')).rejects.toThrow();
  await expect(port.reserveModelContext({ ...context, receiptIds: ['foreign'] })).rejects.toThrow();
  await expect(port.reserveModelContext({ ...context, manifestId: 'foreign' })).rejects.toThrow();
  expect(host.invoke).toHaveBeenCalledTimes(calls);
  await port.reserveModelContext(context);
  const reservedCalls = host.invoke.mock.calls.length;
  await expect(port.reserveModelContext({ ...context, memoryItemIds: ['new'] })).rejects.toThrow();
  expect(host.invoke).toHaveBeenCalledTimes(reservedCalls);
  await port.close();
});
it('discards malformed admission and never opens in web or after cancellation', async () => {
  host.invoke.mockResolvedValueOnce({
    ...descriptor(),
    manifest: { ...descriptor().manifest, id: 'foreign' },
  });
  await expect(openNativePersona(request)).rejects.toThrow();
  expect(host.invoke).toHaveBeenLastCalledWith('close_memory_session', {
    sessionId: descriptor().id,
  });
  const count = host.invoke.mock.calls.length;
  host.desktop = false;
  await expect(openNativePersona(request)).rejects.toThrow();
  host.desktop = true;
  const cancel = new AbortController();
  cancel.abort();
  await expect(openNativePersona(request, cancel.signal)).rejects.toThrow();
  expect(host.invoke).toHaveBeenCalledTimes(count);
});
it('discards a late context and serializes operations while cancellation closes the session', async () => {
  const port = await openNativePersona(request);
  const pending = deferred<unknown>();
  host.invoke.mockReturnValueOnce(pending.promise);
  const cancel = new AbortController();
  const running = port.reserveModelContext(context, cancel.signal);
  const rejected = expect(running).rejects.toThrow();
  await expect(port.recall('query', 'facts')).rejects.toThrow();
  cancel.abort();
  pending.resolve(lease());
  await rejected;
  await expect(port.reserveModelContext(context)).rejects.toThrow();
  expect(host.invoke).toHaveBeenLastCalledWith('close_memory_session', {
    sessionId: descriptor().id,
  });
});
it.each(['id', 'calls', 'bytes', 'fixedEvidence'])(
  'closes on reused or inconsistent model reservation: %s',
  async (kind) => {
    const base = descriptor();
    host.invoke.mockResolvedValueOnce({
      ...base,
      manifest: { ...base.manifest, mode: 'fixedContext' },
    });
    const port = await openNativePersona(request);
    const first = await port.reserveModelContext(context);
    const repair = {
      ...context,
      task: context.task + 'Исправь формат.',
      formatAttempt: 2 as const,
    };
    const second = {
      ...lease([], repair),
      id: 'lease:2',
      usage: { modelCalls: 2, inputBytes: first.inputBytes + lease([], repair).inputBytes },
    };
    const bad =
      kind === 'id'
        ? { ...second, id: first.id }
        : kind === 'calls'
          ? { ...second, usage: { ...second.usage, modelCalls: 1 } }
          : kind === 'bytes'
            ? { ...second, usage: { ...second.usage, inputBytes: second.inputBytes } }
            : { ...second, contextSha256: 'c'.repeat(64) };
    host.invoke.mockResolvedValueOnce(bad);
    await expect(port.reserveModelContext(repair)).rejects.toThrow();
    expect(host.invoke).toHaveBeenLastCalledWith('close_memory_session', { sessionId: base.id });
    await expect(port.reserveModelContext(context)).rejects.toThrow();
  },
);
it('uses a fresh lease and cumulative budgets for format repair while retaining fixed evidence', async () => {
  const base = descriptor();
  host.invoke.mockResolvedValueOnce({
    ...base,
    manifest: { ...base.manifest, mode: 'fixedContext' },
  });
  const port = await openNativePersona(request);
  const first = await port.reserveModelContext(context);
  const repair = { ...context, task: 'Исправь формат.\r\n', formatAttempt: 2 as const };
  const next = {
    ...lease([], repair),
    id: 'lease:2',
    usage: { modelCalls: 2, inputBytes: first.inputBytes + lease([], repair).inputBytes },
  };
  host.invoke.mockResolvedValueOnce(next);
  expect(await port.reserveModelContext(repair)).toEqual(next);
  await port.close();
});
it('cancels opening before admission, cleans up the late native handle and all retry timers', async () => {
  vi.useFakeTimers();
  const pending = deferred<unknown>();
  host.invoke.mockReturnValueOnce(pending.promise);
  const cancel = new AbortController();
  const opened = openNativePersona(request, cancel.signal);
  const rejected = expect(opened).rejects.toThrow('opening stopped');
  cancel.abort(new Error('opening stopped'));
  await vi.advanceTimersByTimeAsync(250);
  expect(
    host.invoke.mock.calls.filter(([name]) => name === 'cancel_ingestion_operation').length,
  ).toBeGreaterThan(1);
  pending.resolve(descriptor());
  await rejected;
  expect(host.invoke).toHaveBeenLastCalledWith('close_memory_session', {
    sessionId: descriptor().id,
  });
  expect(vi.getTimerCount()).toBe(0);
});
it('retains exact source text and attribution for episode actions without accepting caller mutations', async () => {
  const port = await openNativePersona(request);
  const { text, ...document } = sourceFixture.sourceResult.documents[0]!;
  const span = {
    documentId: document.id,
    revisionId: 'revision:1',
    unit: 'utf8Bytes' as const,
    start: 0,
    end: new TextEncoder().encode(text).length,
  };
  const record: Retrieved = {
    receipt: {
      ...retrieval().receipt,
      operation: 'readEpisode',
      sourceBytes: span.end,
      contextTokens: 20,
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
    },
    sources: [
      decodeSourceExcerpt({
        document: { ...document, partition: 'reference' },
        span,
        text,
        role: 'context',
        speakerRelation: 'unknown',
        canSupportOwnWords: false,
        relations: [],
      }),
    ],
  };
  host.invoke.mockResolvedValueOnce(record);
  const read = await port.readEpisode('episode:1');
  expect(read.sources[0]!.text).toBe(text);
  Object.assign(read.sources[0]!.document, { authorLabel: 'changed by caller' });
  Object.assign(read.sources[0]!, { text: 'fabricated' });
  const selected = { ...context, receiptIds: ['receipt:1'] };
  const native = lease([record], selected);
  host.invoke.mockResolvedValueOnce(native);
  const bound = await port.reserveModelContext(selected);
  expect(bound.sources[0]!.text).toBe(text);
  expect(bound.sources[0]!.document.authorLabel).toBe(document.authorLabel);
  await port.close();
});
it('rejects malformed Unicode and unsupported model actions before IPC', async () => {
  const port = await openNativePersona(request);
  const count = host.invoke.mock.calls.length;
  await expect(port.recall('\uD800', 'facts')).rejects.toThrow();
  await expect(port.readEpisode('episode\n1')).rejects.toThrow();
  await expect(
    port.reserveModelContext({ ...context, purpose: 'research', manifestId: null }),
  ).rejects.toThrow();
  expect(host.invoke).toHaveBeenCalledTimes(count);
  await port.close();
});

const simulationRequest = {
  schemaVersion: 1 as const,
  experimentId: 'experiment:1',
  trialId: 'trial:1',
  contextLeaseId: 'lease:1',
  invocationId: 'person-call',
  answer: '{"action":"reply","messages":["  😀\\r\\n "],"summary":"done","styleConflicts":[]}',
};
const simulationEntry = {
  schemaVersion: 1,
  id: 'simulation:abc123',
  experimentId: 'experiment:1',
  trialId: 'trial:1',
  manifestId: request.manifestId,
  role: 'simulation',
  text: '  😀\r\n ',
  createdAt: '2026-11-09T03:00:00.000Z',
};
it('appends only a reserved Persona lease, preserving native scope and exact raw output', async () => {
  const port = await openNativePersona(request);
  const calls = host.invoke.mock.calls.length;
  await expect(port.appendSimulation(simulationRequest)).rejects.toThrow();
  expect(host.invoke).toHaveBeenCalledTimes(calls);
  await port.reserveModelContext(context);
  host.invoke.mockResolvedValueOnce(simulationEntry);
  expect(await port.appendSimulation(simulationRequest)).toEqual(simulationEntry);
  expect(host.invoke).toHaveBeenLastCalledWith('append_persona_simulation', {
    sessionId: descriptor().id,
    request: simulationRequest,
  });
  host.invoke.mockResolvedValueOnce(simulationEntry);
  expect(await port.appendSimulation(simulationRequest)).toEqual(simulationEntry);
  await port.close();
});
it('closes the session on a forged simulation receipt', async () => {
  const port = await openNativePersona(request);
  await port.reserveModelContext(context);
  host.invoke.mockResolvedValueOnce({ ...simulationEntry, role: 'user' });
  await expect(port.appendSimulation(simulationRequest)).rejects.toThrow();
  expect(host.invoke).toHaveBeenLastCalledWith('close_memory_session', {
    sessionId: descriptor().id,
  });
});
it('discards a late saved simulation after cancellation and prevents further writes', async () => {
  const port = await openNativePersona(request);
  await port.reserveModelContext(context);
  const pending = deferred<unknown>();
  host.invoke.mockReturnValueOnce(pending.promise);
  const cancel = new AbortController();
  const running = port.appendSimulation(simulationRequest, cancel.signal);
  const rejected = expect(running).rejects.toThrow();
  await expect(port.recall('query', 'facts')).rejects.toThrow();
  cancel.abort();
  pending.resolve(simulationEntry);
  await rejected;
  await expect(port.appendSimulation(simulationRequest)).rejects.toThrow();
  expect(host.invoke).toHaveBeenLastCalledWith('close_memory_session', {
    sessionId: descriptor().id,
  });
});
