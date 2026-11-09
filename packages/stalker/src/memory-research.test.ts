import { expect, it, vi } from 'vitest';
import type {
  ModelContextLease,
  ResearchMemoryPort,
  ResearchPublicationReceipt,
  RetrievalReceipt,
  SourceExcerpt,
} from '@proteus-int/contracts';
import type { IntelligenceProvider } from '@proteus-int/intelligence';
import { researchMemory } from './memory-research';
import type { MemoryResearchInput } from './research-plan';
import { memoryExcerpt } from './testing/memory-fixtures';

function fixture() {
  const input: MemoryResearchInput & { samples: NonNullable<MemoryResearchInput['samples']> } = {
    person: { label: 'Анна', context: 'Коллега Бориса', publicUrls: [] },
    request: {
      schemaVersion: 1,
      operationId: 'operation:1',
      jobId: 'job:1',
      snapshotId: 'corpus:1',
      personaId: 'persona:1',
      mode: 'lexical',
      period: { from: null, to: null, includeUndated: true },
      recallBudget: {
        maxCalls: 10,
        maxCandidates: 100,
        maxEpisodes: 100,
        maxSourceBytes: 100000,
        maxContextTokens: 10000,
        maxDurationMs: 60000,
      },
      modelBudget: { maxCalls: 8, maxInputBytes: 100000, maxDurationMs: 60000 },
      model: { provider: 'codex', model: 'test-model', effort: 'medium' },
    },
    snapshot: {
      schemaVersion: 1,
      id: 'corpus:1',
      corpusId: 'collection:1',
      parentSnapshotId: null,
      createdAt: '2026-11-09T03:00:00Z',
      bindingIds: ['binding:1'],
      documentRefs: [{ documentId: 'message-1', revisionId: 'revision-1' }],
      episodeIds: ['episode:1', 'episode:2', 'episode:3'],
      splitVersion: 'split:1',
      generation: 1,
      coverage: {
        loadedDocuments: 3,
        indexedDocuments: 3,
        analyzedDocuments: 0,
        usedDocuments: 0,
        totalDocuments: 3,
        loadedBytes: 10000,
        complete: true,
        limitations: [],
      },
    },
    samples: [{ episodeId: 'episode:1', sourceKey: 'source:1', periodKey: '2026-01' }],
    limits: { maxSeedEpisodes: 3, maxPlanningSteps: 1, maxItemsPerBatch: 100 },
  };
  const records = new Map<
    string,
    { receipt: RetrievalReceipt; sources: readonly SourceExcerpt[] }
  >();
  let reads = 0;
  let calls = 0;
  const source = memoryExcerpt();
  const trace = {
    provider: 'codex' as const,
    reasoningMs: 1,
    terminalMs: 1,
    summary: 'Synthetic test',
  };
  const item = {
    draftId: 'model:1',
    kind: 'episode',
    text: source.text,
    sourceSpans: [source.span],
    eventAt: null,
    support: 'observed',
    contradictions: [],
    limitations: [],
  };
  const run = vi.fn<IntelligenceProvider['run']>().mockImplementation((request) =>
    Promise.resolve({
      answer: request.systemPrompt.includes('STALKER memory planning')
        ? JSON.stringify({ action: 'finish', reason: 'Достаточно для ограниченной выборки' })
        : JSON.stringify({ items: [item] }),
      trace,
    }),
  );
  const provider: IntelligenceProvider = { id: 'codex', run };
  function retrieved(episodeId: string, operation: 'recall' | 'readEpisode') {
    const receipt: RetrievalReceipt = {
      schemaVersion: 2,
      id: `receipt:${++reads}`,
      operation,
      snapshotId: input.request.snapshotId,
      policyVersion: 'policy:1',
      retrievalVersion: 'nativeMemoryRetrieval:1',
      indexGeneration: 1,
      embeddingFingerprint: null,
      mode: 'lexical',
      elapsedMs: 1,
      sourceBytes: new TextEncoder().encode(source.text).length,
      contextTokens: 20,
      hits: [
        {
          chunkId: `chunk:${reads}`,
          episodeId,
          spans: [source.span],
          lexicalRank: operation === 'recall' ? 1 : null,
          semanticRank: null,
          fusionScore: operation === 'recall' ? 1 / 61 : 0,
        },
      ],
    };
    const record = { receipt, sources: [source] };
    records.set(receipt.id, record);
    return record;
  }
  const recall = vi
    .fn<ResearchMemoryPort['recall']>()
    .mockImplementation(() =>
      Promise.resolve({ ...retrieved('episode:1', 'recall'), items: [], limitations: [] }),
    );
  const read = vi
    .fn<ResearchMemoryPort['readEpisode']>()
    .mockImplementation((episodeId) =>
      Promise.resolve({ ...retrieved(episodeId, 'readEpisode'), episodeId }),
    );
  const reserve = vi
    .fn<ResearchMemoryPort['reserveModelContext']>()
    .mockImplementation((request) => {
      const sources = request.receiptIds.flatMap((id) => records.get(id)?.sources ?? []);
      const task = request.task + '\nNative sources: ' + JSON.stringify(sources);
      const inputBytes = new TextEncoder().encode(
        JSON.stringify([request.systemPrompt, task]),
      ).length;
      const result: ModelContextLease = {
        schemaVersion: 2,
        id: `lease:${++calls}`,
        singleUse: true,
        personaId: input.request.personaId,
        inputSha256: 'a'.repeat(64),
        inputBytes,
        systemPrompt: request.systemPrompt,
        task,
        corpusSnapshotId: input.request.snapshotId,
        manifestId: null,
        purpose: 'research',
        formatAttempt: request.formatAttempt,
        policyVersion: 'policy:1',
        receiptIds: request.receiptIds,
        items: [],
        sources,
        contextSha256: 'b'.repeat(64),
        expiresAt: '2099-01-01T00:00:00.000Z',
        usage: { modelCalls: calls, inputBytes: inputBytes * calls },
      };
      return Promise.resolve(result);
    });
  const publish = vi.fn<ResearchMemoryPort['publishResearch']>().mockImplementation((draft) => {
    const publishedAt = '2026-11-09T03:00:00.000Z';
    const itemIds = draft.items.map((item, index) => ({
      draftId: item.draftId,
      itemId: `native:${index}`,
    }));
    const receipt: ResearchPublicationReceipt = {
      schemaVersion: 1,
      id: 'publication:1',
      snapshot: {
        schemaVersion: 1,
        id: 'memory:1',
        corpusSnapshotId: input.request.snapshotId,
        extractionVersion: draft.extractionVersion,
        itemIds: itemIds.map((item) => item.itemId),
        createdAt: publishedAt,
      },
      itemIds,
      analyzedSpans: draft.analyzedRefs.length ? [source.span] : [],
      publishedAt,
      coverage: {
        ...input.snapshot.coverage,
        analyzedDocuments: draft.analyzedRefs.length,
        usedDocuments: draft.items.length ? 1 : 0,
      },
    };
    return Promise.resolve(receipt);
  });
  const close = vi.fn<ResearchMemoryPort['close']>().mockResolvedValue();
  const getCatalog = vi.fn<ResearchMemoryPort['getResearchCatalog']>();
  const memory: ResearchMemoryPort = {
    getResearchCatalog: getCatalog,
    session: {
      schemaVersion: 1,
      id: 'session:1',
      personaId: input.request.personaId,
      corpusSnapshotId: input.request.snapshotId,
      policyVersion: 'policy:1',
      mode: 'lexical',
      indexGeneration: 1,
      expiresAt: '2099-01-01T00:00:00.000Z',
    },
    recall,
    readEpisode: read,
    reserveModelContext: reserve,
    publishResearch: publish,
    close,
  };
  const open = vi.fn().mockResolvedValue(memory);
  return {
    input,
    provider,
    memory,
    open,
    run,
    read,
    recall,
    reserve,
    publish,
    close,
    getCatalog,
    source,
    item,
    trace,
  };
}

it('performs task-free sampling, native-bound planning, tools-off extraction and durable publication', async () => {
  const f = fixture();
  const result = await researchMemory(f.input, f.provider, f.open);
  expect(f.read.mock.calls.map(([id]) => id)).toEqual(['episode:1']);
  expect(f.reserve).toHaveBeenCalledTimes(2);
  expect(f.run).toHaveBeenCalledTimes(2);
  for (const [request] of f.run.mock.calls) {
    expect(request.operation).toBe('corpus-analysis');
    expect(request.sourceContext).toBe(true);
    expect(request.memoryContext?.sessionId).toBe('session:1');
    expect(request.task).toContain('Анна');
    expect(request.task).toContain('Борис');
    expect(request.task).toContain(JSON.stringify(f.source.text).slice(1, -1));
  }
  const submitted = f.publish.mock.calls[0]![0];
  expect(submitted.items[0]?.text).toBe(f.source.text);
  expect(submitted.items[0]?.sourceSpans).toEqual([f.source.span]);
  expect(submitted.items[0]?.episodeIds).toEqual(['episode:1']);
  expect(submitted.analyzedRefs).toEqual([
    { documentId: f.source.span.documentId, revisionId: f.source.span.revisionId },
  ]);
  expect(submitted.contextLeaseIds).toEqual(['lease:1', 'lease:2']);
  expect(result.publication.coverage).toMatchObject({
    loadedDocuments: 3,
    indexedDocuments: 3,
    analyzedDocuments: 1,
    usedDocuments: 1,
    complete: true,
  });
  expect(result.traces).toEqual([f.trace, f.trace]);
  expect(f.close).toHaveBeenCalledOnce();
});
it('samples across source/period groups before taking another seed from one group', async () => {
  const f = fixture();
  const input = {
    ...f.input,
    samples: [
      { episodeId: 'episode:1', sourceKey: 'a', periodKey: '2026-01' },
      { episodeId: 'episode:2', sourceKey: 'a', periodKey: '2026-01' },
      { episodeId: 'episode:3', sourceKey: 'b', periodKey: '2026-02' },
    ],
    limits: { ...f.input.limits, maxSeedEpisodes: 2, maxPlanningSteps: 0 },
  };
  f.run.mockResolvedValue({ answer: '{"items":[]}', trace: f.trace });
  await researchMemory(input, f.provider, f.open);
  expect(f.read.mock.calls.map(([id]) => id)).toEqual(['episode:1', 'episode:3']);
});
it('dispatches only validated follow-up recall and refuses unknown episode IDs', async () => {
  const f = fixture();
  f.run.mockResolvedValueOnce({
    answer: '{"action":"recall","query":"противоречие","intent":"context"}',
    trace: f.trace,
  });
  await researchMemory(f.input, f.provider, f.open);
  expect(f.recall).toHaveBeenCalledWith('противоречие', 'context', expect.any(AbortSignal));
  const blocked = fixture();
  blocked.run.mockResolvedValue({
    answer: '{"action":"readEpisode","episodeId":"foreign"}',
    trace: blocked.trace,
  });
  await expect(researchMemory(blocked.input, blocked.provider, blocked.open)).rejects.toThrow();
  expect(blocked.read).toHaveBeenCalledTimes(1);
  expect(blocked.publish).not.toHaveBeenCalled();
  expect(blocked.close).toHaveBeenCalledOnce();
});
it('cannot expand tools or scope through model actions or source instructions', async () => {
  const f = fixture();
  f.run.mockResolvedValue({
    answer: '{"action":"publishResearch","grantId":"forged","items":[]}',
    trace: f.trace,
  });
  await expect(researchMemory(f.input, f.provider, f.open)).rejects.toThrow();
  expect(f.run).toHaveBeenCalledTimes(2);
  expect(f.publish).not.toHaveBeenCalled();
  expect(f.close).toHaveBeenCalledOnce();
});
it('does not publish unsupported extraction after two bounded format attempts', async () => {
  const f = fixture();
  const input = { ...f.input, limits: { ...f.input.limits, maxPlanningSteps: 0 } };
  f.run.mockResolvedValue({
    answer: JSON.stringify({ items: [{ ...f.item, text: 'unsupported invented quotation' }] }),
    trace: f.trace,
  });
  await expect(researchMemory(input, f.provider, f.open)).rejects.toThrow();
  expect(f.run).toHaveBeenCalledTimes(2);
  expect(f.publish).not.toHaveBeenCalled();
  expect(f.close).toHaveBeenCalledOnce();
});
it('reserves model attempts for extraction instead of letting planning exhaust the budget', async () => {
  const f = fixture();
  const input = {
    ...f.input,
    request: { ...f.input.request, modelBudget: { ...f.input.request.modelBudget, maxCalls: 2 } },
  };
  const result = await researchMemory(input, f.provider, f.open);
  expect(f.run).toHaveBeenCalledTimes(1);
  expect(f.run.mock.calls[0]![0].systemPrompt).toContain('STALKER memory extraction');
  expect(result.publication.itemIds).toHaveLength(1);
});
it('rejects invalid seed scope and host limits before opening native resources', async () => {
  const f = fixture();
  for (const input of [
    { ...f.input, samples: [{ ...f.input.samples[0]!, episodeId: 'foreign' }] },
    { ...f.input, snapshot: { ...f.input.snapshot, id: 'foreign' } },
    { ...f.input, limits: { ...f.input.limits, maxSeedEpisodes: 0 } },
  ])
    await expect(researchMemory(input, f.provider, f.open)).rejects.toThrow();
  expect(f.open).not.toHaveBeenCalled();
});
it('never publishes after cancellation during a read and closes the native session', async () => {
  const f = fixture();
  const controller = new AbortController();
  const original = f.read.getMockImplementation()!;
  f.read.mockImplementation((id, signal) => {
    controller.abort(new Error('stop research'));
    return original(id, signal);
  });
  await expect(researchMemory(f.input, f.provider, f.open, controller.signal)).rejects.toThrow(
    'stop research',
  );
  expect(f.run).not.toHaveBeenCalled();
  expect(f.publish).not.toHaveBeenCalled();
  expect(f.close).toHaveBeenCalledOnce();
});

it('uses the parser action schema for contradictions and maps local IDs to one publication namespace', async () => {
  const f = fixture();
  const input = { ...f.input, limits: { ...f.input.limits, maxPlanningSteps: 0 } };
  const first = {
    ...f.item,
    draftId: 'a',
    kind: 'reportedAssertion',
    support: 'conflicting',
    contradictions: [{ kind: 'draft', itemId: 'b' }],
  };
  const second = { ...first, draftId: 'b', contradictions: [{ kind: 'draft', itemId: 'a' }] };
  f.run.mockImplementation((request) => {
    expect(request.systemPrompt).toContain('kind:"draft",itemId:string');
    return Promise.resolve({ answer: JSON.stringify({ items: [first, second] }), trace: f.trace });
  });
  await researchMemory(input, f.provider, f.open);
  const items = f.publish.mock.calls[0]![0].items;
  expect(items.map((item) => item.support)).toEqual(['conflicting', 'conflicting']);
  expect(items[0]?.contradictions).toEqual([{ kind: 'draft', draftId: items[1]?.draftId }]);
  expect(items[1]?.contradictions).toEqual([{ kind: 'draft', draftId: items[0]?.draftId }]);
  expect(items[0]?.text).toBe(f.source.text);
});
it('preserves a context author as reported evidence and rejects attributing their words to the selected author', async () => {
  for (const ownClaim of [true, false]) {
    const f = fixture();
    Object.assign(f.source, {
      role: 'context',
      speakerRelation: 'context',
      canSupportOwnWords: false,
    });
    Object.assign(f.source.document, {
      authorship: 'other',
      authorId: 'boris',
      authorLabel: 'Борис',
    });
    const input = { ...f.input, limits: { ...f.input.limits, maxPlanningSteps: 0 } };
    const item = ownClaim ? f.item : { ...f.item, kind: 'reportedAssertion', support: 'reported' };
    f.run.mockResolvedValue({ answer: JSON.stringify({ items: [item] }), trace: f.trace });
    if (ownClaim) {
      await expect(researchMemory(input, f.provider, f.open)).rejects.toThrow();
      expect(f.publish).not.toHaveBeenCalled();
    } else {
      await researchMemory(input, f.provider, f.open);
      expect(f.publish.mock.calls[0]![0].items[0]?.support).toBe('reported');
      expect(f.run.mock.calls[0]![0].task).toContain('Борис');
    }
    expect(f.close).toHaveBeenCalledOnce();
  }
});
it('rejects oversized combined persona input before opening instead of silently trimming exact text', async () => {
  const f = fixture();
  const input = {
    ...f.input,
    person: {
      ...f.input.person,
      publicUrls: Array<string>(20).fill('https://example.org/' + 'a'.repeat(1980)),
    },
  };
  await expect(researchMemory(input, f.provider, f.open)).rejects.toThrow();
  expect(f.open).not.toHaveBeenCalled();
});

it('reports a partial seed catalog and an exhausted planning limit separately from acquisition completeness', async () => {
  const f = fixture();
  f.run.mockResolvedValueOnce({
    answer: '{"action":"recall","query":"подтверждение","intent":"context"}',
    trace: f.trace,
  });
  const result = await researchMemory(f.input, f.provider, f.open);
  expect(result.limitations).toEqual(
    expect.arrayContaining([
      expect.stringContaining('Начальная выборка'),
      expect.stringContaining('Предел поисковых шагов'),
    ]),
  );
  expect(result.publication.coverage.complete).toBe(true);
  expect(result.publication.coverage.analyzedDocuments).toBe(1);
});

it('retains both the research failure and failed session cleanup', async () => {
  const f = fixture();
  const primary = new Error('source access revoked');
  const cleanup = new Error('close failed');
  f.read.mockRejectedValue(primary);
  f.close.mockRejectedValue(cleanup);
  const error = await researchMemory(f.input, f.provider, f.open).catch((cause: unknown) => cause);
  expect(error).toBeInstanceOf(AggregateError);
  expect((error as AggregateError).errors).toEqual([primary, cleanup]);
  expect(f.publish).not.toHaveBeenCalled();
});

it('closes a timed out research without invoking a model or publishing', async () => {
  vi.useFakeTimers();
  try {
    const f = fixture();
    f.read.mockImplementation(
      (_id, signal) =>
        new Promise((_resolve, reject) => {
          signal?.addEventListener(
            'abort',
            () =>
              reject(
                signal.reason instanceof Error ? signal.reason : new Error('research aborted'),
              ),
            { once: true },
          );
        }),
    );
    const outcome = researchMemory(f.input, f.provider, f.open);
    const rejected = expect(outcome).rejects.toThrow('Истёк срок исследования.');
    await vi.advanceTimersByTimeAsync(60000);
    await rejected;
    expect(f.close).toHaveBeenCalledOnce();
    expect(f.run).not.toHaveBeenCalled();
    expect(f.publish).not.toHaveBeenCalled();
  } finally {
    vi.useRealTimers();
  }
});

it('obtains initial candidates from the authorized native catalog when host seeds are absent', async () => {
  const f = fixture();
  const { samples, ...input } = f.input;
  const catalog = {
    schemaVersion: 1 as const,
    snapshotId: f.memory.session.corpusSnapshotId,
    indexGeneration: f.memory.session.indexGeneration,
    personaId: f.memory.session.personaId,
    policyVersion: f.memory.session.policyVersion,
    eligibleEpisodes: samples.length,
    samples: samples.map((sample) => ({ ...sample, periodKey: 'window30d:681' })),
  };
  f.getCatalog.mockResolvedValue(catalog);
  const result = await researchMemory(input, f.provider, f.open);
  expect(f.getCatalog).toHaveBeenCalledExactlyOnceWith(10000, expect.any(AbortSignal));
  expect(f.read).toHaveBeenCalledWith(samples[0]!.episodeId, expect.any(AbortSignal));
  expect(result.publication.coverage.analyzedDocuments).toBe(1);
  expect(f.close).toHaveBeenCalledOnce();
});

it('rejects a foreign catalog before reading sources or invoking a model and closes the session', async () => {
  const f = fixture();
  const { samples, ...input } = f.input;
  f.getCatalog.mockResolvedValue({
    schemaVersion: 1,
    snapshotId: 'foreign',
    indexGeneration: 1,
    personaId: f.memory.session.personaId,
    policyVersion: f.memory.session.policyVersion,
    eligibleEpisodes: samples.length,
    samples,
  });
  await expect(researchMemory(input, f.provider, f.open)).rejects.toThrow();
  expect(f.read).not.toHaveBeenCalled();
  expect(f.run).not.toHaveBeenCalled();
  expect(f.close).toHaveBeenCalledOnce();
});
