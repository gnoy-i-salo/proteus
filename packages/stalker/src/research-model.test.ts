import { beforeEach, expect, it, vi } from 'vitest';
import type {
  IntelligenceRequest,
  ModelContextLease,
  ModelContextRequest,
  OpenResearchSessionRequest,
  ResearchMemoryPort,
} from '@proteus-int/contracts';
import type { IntelligenceProvider } from '@proteus-int/intelligence';
import { structured } from '@proteus-int/intelligence';
import { createResearchModel } from './research-model';
import { memoryExcerpt } from './testing/memory-fixtures';

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
  model: { provider: 'codex', model: 'bound-model', effort: 'high' },
};
const base: IntelligenceRequest = {
  systemPrompt: '  Инструкция\r\n',
  task: '  Точная задача 👩🏽‍🔬\n',
  operation: 'corpus-analysis',
  effort: 'low',
  formatAttempt: 1,
};
const trace = {
  provider: 'codex' as const,
  reasoningMs: 1,
  terminalMs: 2,
  summary: 'Открытое описание',
  reasoning: [{ id: 'r1', kind: 'summary' as const, text: 'Публичный текст CLI\r\n' }],
};
const run = vi.fn<IntelligenceProvider['run']>();
const reserve = vi.fn<ResearchMemoryPort['reserveModelContext']>();
const onValidation = vi.fn<NonNullable<IntelligenceProvider['onValidation']>>();
const provider: IntelligenceProvider = { id: 'codex', run, onValidation };
const memory: ResearchMemoryPort = {
  getResearchCatalog: vi.fn(),
  session: {
    schemaVersion: 1,
    id: 'session:1',
    personaId: request.personaId,
    corpusSnapshotId: request.snapshotId,
    policyVersion: 'policy:1',
    mode: request.mode,
    indexGeneration: 1,
    expiresAt: '2026-11-09T03:01:00.000Z',
  },
  recall: vi.fn(),
  readEpisode: vi.fn(),
  reserveModelContext: reserve,
  publishResearch: vi.fn(),
  close: vi.fn(),
};
function lease(input: ModelContextRequest, calls: number): ModelContextLease {
  const task = input.task + '\nNative original sources';
  const inputBytes = new TextEncoder().encode(JSON.stringify([input.systemPrompt, task])).length;
  return {
    schemaVersion: 2,
    id: `lease:${calls}`,
    singleUse: true,
    personaId: request.personaId,
    inputSha256: 'a'.repeat(64),
    inputBytes,
    systemPrompt: input.systemPrompt,
    task,
    corpusSnapshotId: request.snapshotId,
    manifestId: null,
    purpose: 'research',
    formatAttempt: input.formatAttempt,
    policyVersion: memory.session.policyVersion,
    receiptIds: input.receiptIds,
    items: [],
    sources: [],
    contextSha256: 'b'.repeat(64),
    expiresAt: memory.session.expiresAt,
    usage: { modelCalls: calls, inputBytes: inputBytes * calls },
  };
}
beforeEach(() => {
  vi.resetAllMocks();
  let calls = 0;
  reserve.mockImplementation((input) => Promise.resolve(lease(input, ++calls)));
  run.mockResolvedValue({ invocationId: 'invocation:1', answer: '{"items":[]}', trace });
});
it('reserves the exact host base input and dispatches only the native assembled strings with fixed model settings', async () => {
  const model = createResearchModel(request, memory, provider);
  const bound = model.forReceipts(['receipt:1']);
  await bound.run(base);
  const selected: ModelContextRequest = {
    schemaVersion: 2,
    purpose: 'research',
    manifestId: null,
    systemPrompt: base.systemPrompt,
    task: base.task,
    receiptIds: ['receipt:1'],
    memoryItemIds: [],
    formatAttempt: 1,
  };
  expect(reserve).toHaveBeenCalledExactlyOnceWith(selected, undefined);
  expect(run).toHaveBeenCalledExactlyOnceWith(
    {
      ...base,
      task: lease(selected, 1).task,
      model: 'bound-model',
      effort: 'high',
      sourceContext: true,
      memoryContext: { sessionId: 'session:1', leaseId: 'lease:1' },
    },
    undefined,
  );
  expect(model.completedLeaseIds).toEqual(['lease:1']);
  expect(model.traces).toEqual([trace]);
  expect(model.remainingCalls).toBe(2);
});
it('keeps format repair on new single-use leases and forwards validation events for both attempts', async () => {
  const model = createResearchModel(request, memory, provider);
  run.mockResolvedValueOnce({ invocationId: 'bad:1', answer: 'not JSON', trace });
  const result = await structured(
    model.forReceipts(['receipt:1']),
    'STALKER',
    'Extract items',
    { label: 'Анна' },
    undefined,
    'medium',
    (value) => value,
    'corpus-analysis',
    true,
  );
  expect(result.data).toEqual({ items: [] });
  expect(reserve.mock.calls.map(([input]) => input.formatAttempt)).toEqual([1, 2]);
  expect(run.mock.calls.map(([input]) => input.memoryContext?.leaseId)).toEqual([
    'lease:1',
    'lease:2',
  ]);
  expect(model.completedLeaseIds).toEqual(['lease:1', 'lease:2']);
  expect(onValidation.mock.calls.map(([event]) => event.status)).toEqual(['rejected', 'accepted']);
});
it('shares one attempt budget across planning, extraction and failed reservations', async () => {
  const model = createResearchModel(
    { ...request, modelBudget: { ...request.modelBudget, maxCalls: 2 } },
    memory,
    provider,
  );
  reserve.mockRejectedValueOnce(new Error('reservation refused'));
  await expect(model.forReceipts([]).run(base)).rejects.toThrow('reservation refused');
  await model.forReceipts([]).run(base);
  await expect(model.forReceipts([]).run(base)).rejects.toThrow('бюджет');
  expect(reserve).toHaveBeenCalledTimes(2);
  expect(run).toHaveBeenCalledTimes(1);
});
it('does not mark a failed CLI call as completed and keeps prior traces detached', async () => {
  const model = createResearchModel(request, memory, provider);
  run.mockRejectedValueOnce(new Error('CLI failed'));
  await expect(model.forReceipts([]).run(base)).rejects.toThrow('CLI failed');
  expect(model.completedLeaseIds).toEqual([]);
  await model.forReceipts([]).run(base);
  const exposed = model.traces;
  Object.assign(exposed[0]!, { summary: 'changed' });
  expect(model.traces[0]?.summary).toBe(trace.summary);
  expect(model.completedLeaseIds).toEqual(['lease:2']);
});
it('rejects a provider/session mismatch and nonresearch operations before reservation', async () => {
  expect(() => createResearchModel(request, memory, { ...provider, id: 'kimi' })).toThrow();
  expect(() =>
    createResearchModel(
      request,
      { ...memory, session: { ...memory.session, personaId: 'foreign' } },
      provider,
    ),
  ).toThrow();
  const model = createResearchModel(request, memory, provider);
  await expect(
    model.forReceipts([]).run({ ...base, operation: 'public-research' }),
  ).rejects.toThrow();
  await expect(
    model
      .forReceipts([])
      .run({ ...base, memoryContext: { sessionId: 'forged', leaseId: 'forged' } }),
  ).rejects.toThrow();
  expect(reserve).not.toHaveBeenCalled();
});
it('never sends a model input after cancellation during reservation', async () => {
  const controller = new AbortController();
  reserve.mockImplementation((input) => {
    controller.abort(new Error('stop reservation'));
    return Promise.resolve(lease(input, 1));
  });
  const model = createResearchModel(request, memory, provider);
  await expect(model.forReceipts([]).run(base, controller.signal)).rejects.toThrow(
    'stop reservation',
  );
  expect(run).not.toHaveBeenCalled();
  expect(model.completedLeaseIds).toEqual([]);
});

it('accounts only source revisions from completed calls, deduplicating exact tuples without conflating revisions', async () => {
  const source = memoryExcerpt();
  const edited = memoryExcerpt({ revisionId: 'revision:2' });
  let calls = 0;
  reserve.mockImplementation((input) =>
    Promise.resolve({ ...lease(input, ++calls), sources: [source, source, edited] }),
  );
  const model = createResearchModel(request, memory, provider);
  run.mockRejectedValueOnce(new Error('CLI failed'));
  await expect(model.forReceipts(['receipt:1']).run(base)).rejects.toThrow();
  expect(model.analyzedRefs).toEqual([]);
  await model.forReceipts(['receipt:1']).run(base);
  expect(model.analyzedRefs).toEqual([
    { documentId: source.span.documentId, revisionId: source.span.revisionId },
    { documentId: edited.span.documentId, revisionId: edited.span.revisionId },
  ]);
  const refs = model.analyzedRefs;
  Object.assign(refs[0]!, { documentId: 'changed' });
  expect(model.analyzedRefs[0]?.documentId).toBe(source.span.documentId);
});
