import { beforeEach, expect, it, vi } from 'vitest';
import type { PersonaMemoryPort, MemoryItem, SkillDefinition } from '@proteus-int/contracts';
import type { IntelligenceProvider } from '@proteus-int/intelligence';
import { solveMemory } from './memory-person';
import { personRuntime } from './solve-task';
import {
  request,
  session,
  profile,
  lease,
  retrieval,
  sourceRetrieval,
  snapshotOf,
} from './testing/person-memory-fixtures';
const input = {
  request,
  profile,
  task: '  Напиши Борису\r\n🙂  ',
  maxRetrievalSteps: 3,
  simulation: { experimentId: 'experiment:1', trialId: 'trial:1' },
};
const messages = ['  Ну… **да** 👩🏽‍🔬\r\n', 'é\tне менять  '];
const reply = { action: 'reply', messages, summary: 'Готово', styleConflicts: [] };
const trace = { provider: 'codex' as const, reasoningMs: 1, terminalMs: 2, summary: 'CLI summary' };
const run = vi.fn<IntelligenceProvider['run']>();
const reserve = vi.fn<PersonaMemoryPort['reserveModelContext']>();
const recall = vi.fn<PersonaMemoryPort['recall']>();
const readEpisode = vi.fn<PersonaMemoryPort['readEpisode']>();
const close = vi.fn<PersonaMemoryPort['close']>();
const append = vi.fn<PersonaMemoryPort['appendSimulation']>();
const provider: IntelligenceProvider = { id: 'codex', run };
let memory: PersonaMemoryPort;
const open = vi.fn<() => Promise<PersonaMemoryPort>>();
it('passes separate task guidance without changing the frozen profile or evidence selection', async () => {
  const adaptation = {
    schemaVersion: 1 as const,
    manifestId: request.manifestId,
    task: input.task,
    strategies: [
      {
        id: 's1',
        instruction: '  Уточни условия\r\n',
        evidenceIds: [],
        status: 'hypothesis' as const,
        limitations: ['Предполагаемая стратегия, не установленная черта.'],
      },
    ],
  };
  const chosen = { ...input, adaptation };
  await solveMemory(chosen, provider, open);
  const sent: unknown = JSON.parse(reserve.mock.calls[0]![0].task);
  expect(sent).toMatchObject({ taskAdaptation: adaptation, taskSkills: [], skills: [] });
  expect(reserve.mock.calls[0]![0].memoryItemIds).toEqual([]);
  expect(profile.taskSkills).toEqual([]);
  expect(run.mock.calls[0]![0].systemPrompt).toContain('taskAdaptation');
});
it('rejects stale task guidance before opening memory or calling a provider', async () => {
  const chosen = {
    ...input,
    adaptation: {
      schemaVersion: 1 as const,
      manifestId: request.manifestId,
      task: 'other task',
      strategies: [],
    },
  };
  await expect(solveMemory(chosen, provider, open)).rejects.toThrow();
  expect(open).not.toHaveBeenCalled();
  expect(run).not.toHaveBeenCalled();
});
beforeEach(() => {
  vi.resetAllMocks();
  memory = {
    session: session(),
    reserveModelContext: reserve,
    recall,
    readEpisode,
    close,
    appendSimulation: append,
  };
  open.mockImplementation(() => Promise.resolve(memory));
  let calls = 0;
  reserve.mockImplementation((draft) =>
    Promise.resolve(lease(draft, memory.session, ++calls, [retrieval()])),
  );
  recall.mockResolvedValue(retrieval());
  run.mockResolvedValue({ invocationId: 'call-1', answer: JSON.stringify(reply), trace });
  close.mockResolvedValue();
  append.mockImplementation((draft) =>
    Promise.resolve({
      schemaVersion: 1,
      id: 'simulation:abc',
      manifestId: request.manifestId,
      experimentId: draft.experimentId,
      trialId: draft.trialId,
      role: 'simulation',
      text: messages.join('\n\n'),
      createdAt: '2026-11-09T00:00:00.000Z',
    }),
  );
});
it('answers through a pinned native context preserving identity, exact task/messages and observable traces', async () => {
  const result = await solveMemory(input, provider, open);
  const sent = run.mock.calls[0]![0];
  expect(sent).toMatchObject({
    operation: 'persona-reply',
    model: 'pinned-model',
    effort: 'high',
    sourceContext: true,
    memoryContext: { sessionId: 'session:1', leaseId: 'lease:1' },
  });
  expect(sent.systemPrompt).toContain('Respond in first person');
  expect(sent.task).toContain(JSON.stringify(input.task));
  expect(sent.task).toContain(JSON.stringify(profile.label));
  expect(result).toMatchObject({
    answer: messages.join('\n\n'),
    reply: { version: 2, messages, styleConflicts: [] },
    invocationId: 'call-1',
    contextLeaseIds: ['lease:1'],
    traces: [trace],
  });
  expect(close).toHaveBeenCalledOnce();
});
it('executes a bounded recall then re-reserves native evidence for the answer', async () => {
  run.mockResolvedValueOnce({
    answer: JSON.stringify({ action: 'recall', query: '  Борис\r\n', intent: 'context' }),
    trace,
  });
  const result = await solveMemory(input, provider, open);
  expect(recall).toHaveBeenCalledWith('  Борис\r\n', 'context', expect.any(AbortSignal));
  expect(result.retrievals).toEqual([
    {
      action: { action: 'recall', query: '  Борис\r\n', intent: 'context' },
      receipt: retrieval().receipt,
    },
  ]);
  expect(reserve.mock.calls.map(([draft]) => draft.receiptIds)).toEqual([[], ['receipt:1']]);
  expect(result.contextLeaseIds).toEqual(['lease:1', 'lease:2']);
  expect(result.traces).toHaveLength(2);
  expect(close).toHaveBeenCalledOnce();
});
it('uses fresh contexts for format repair and never dispatches model-invented host commands', async () => {
  run.mockResolvedValueOnce({
    answer: JSON.stringify({ action: 'freezeManifest', manifestId: 'forged' }),
    trace,
  });
  await solveMemory(input, provider, open);
  expect(reserve.mock.calls.map(([draft]) => draft.formatAttempt)).toEqual([1, 2]);
  expect(recall).not.toHaveBeenCalled();
  expect(readEpisode).not.toHaveBeenCalled();
});
it('keeps fixed-context and neutral roles without offering or invoking recall', async () => {
  memory = {
    ...memory,
    session: {
      ...memory.session,
      manifest: {
        ...memory.session.manifest,
        mode: 'fixedContext',
        profileSnapshot: snapshotOf({ ...profile, sourceStatus: 'neutral' }),
      },
    },
  };
  run.mockResolvedValueOnce({
    answer: JSON.stringify({ action: 'recall', query: 'x', intent: 'facts' }),
    trace,
  });
  const result = await solveMemory(
    { ...input, profile: { ...profile, sourceStatus: 'neutral' } },
    provider,
    open,
  );
  expect(run.mock.calls[0]![0].systemPrompt).toContain('without adopting a personal identity');
  expect(run.mock.calls[0]![0].systemPrompt).not.toContain('Respond in first person');
  expect(run.mock.calls[0]![0].task).toContain('"personaLabel":null');
  expect(recall).not.toHaveBeenCalled();
  expect(result.contextLeaseIds).toEqual(['lease:1', 'lease:2']);
});
it('rejects memory profiles on the legacy path instead of silently omitting memory', async () => {
  await expect(personRuntime.solve(profile, input.task, provider)).rejects.toThrow(/памят/);
  expect(run).not.toHaveBeenCalled();
});

it('stops before another CLI call when native permission is revoked between turns', async () => {
  run.mockResolvedValueOnce({
    answer: JSON.stringify({ action: 'recall', query: 'Борис', intent: 'facts' }),
    trace,
  });
  recall.mockRejectedValueOnce(new Error('grant revoked'));
  await expect(solveMemory(input, provider, open)).rejects.toThrow('grant revoked');
  expect(run).toHaveBeenCalledOnce();
  expect(reserve).toHaveBeenCalledOnce();
  expect(close).toHaveBeenCalledOnce();
});
it('bounds retrieval steps and reserves two attempts for the final answer', async () => {
  memory = {
    ...memory,
    session: {
      ...memory.session,
      manifest: {
        ...memory.session.manifest,
        modelCallBudget: { ...memory.session.manifest.modelCallBudget, maxCalls: 4 },
      },
    },
  };
  const action = { action: 'recall', query: 'Борис', intent: 'facts' };
  run.mockResolvedValueOnce({ answer: 'invalid JSON', trace });
  run.mockResolvedValueOnce({ answer: JSON.stringify(action), trace });
  run.mockResolvedValueOnce({ answer: JSON.stringify(action), trace });
  const result = await solveMemory({ ...input, maxRetrievalSteps: 1 }, provider, open);
  expect(recall).toHaveBeenCalledOnce();
  expect(run).toHaveBeenCalledTimes(4);
  expect(run.mock.calls[2]![0].task).toContain('"retrievalAllowed":false');
  expect(result.contextLeaseIds).toEqual(['lease:1', 'lease:2', 'lease:3', 'lease:4']);
});
it('does not dispatch unknown episodes or repeated requests even when model proposes them', async () => {
  const action = { action: 'recall', query: 'Борис', intent: 'facts' };
  run.mockResolvedValueOnce({
    answer: JSON.stringify({ action: 'readEpisode', episodeId: 'invented' }),
    trace,
  });
  run.mockResolvedValueOnce({ answer: JSON.stringify(action), trace });
  run.mockResolvedValueOnce({ answer: JSON.stringify(action), trace });
  await solveMemory(input, provider, open);
  expect(readEpisode).not.toHaveBeenCalled();
  expect(recall).toHaveBeenCalledOnce();
  expect(reserve.mock.calls.map(([draft]) => draft.formatAttempt)).toEqual([1, 2, 1, 2]);
});
it('rejects fabricated or foreign receipt scope before another model invocation', async () => {
  run.mockResolvedValueOnce({
    answer: JSON.stringify({ action: 'recall', query: 'Борис', intent: 'facts' }),
    trace,
  });
  recall.mockResolvedValueOnce({
    ...retrieval(),
    receipt: { ...retrieval().receipt, snapshotId: 'foreign' },
  });
  await expect(solveMemory(input, provider, open)).rejects.toThrow();
  expect(run).toHaveBeenCalledOnce();
  expect(close).toHaveBeenCalledOnce();
});
it('does not reuse a native context and rejects changed settings and unsupported profile versions', async () => {
  run.mockResolvedValueOnce({ answer: 'invalid JSON', trace });
  reserve.mockImplementation((draft) => Promise.resolve(lease(draft, memory.session, 1)));
  await expect(solveMemory(input, provider, open)).rejects.toThrow(/контекст|бюджет/);
  expect(run).toHaveBeenCalledOnce();
  await expect(solveMemory(input, { ...provider, id: 'kimi' }, open)).rejects.toThrow(/CLI/);
  memory = {
    ...memory,
    session: {
      ...memory.session,
      manifest: { ...memory.session.manifest, profileVersion: 'unknown:9' },
    },
  };
  await expect(solveMemory(input, provider, open)).rejects.toThrow(/Профиль/);
  expect(run).toHaveBeenCalledOnce();
  expect(close).toHaveBeenCalledTimes(3);
});
it('rejects malformed or mismatched host input before opening memory', async () => {
  const legacy = { ...profile };
  Reflect.deleteProperty(legacy, 'memory');
  for (const bad of [
    { ...input, maxRetrievalSteps: -1 },
    { ...input, maxRetrievalSteps: 65 },
    { ...input, task: '\ud800' },
    { ...input, profile: { ...profile, id: 'foreign' } },
    { ...input, profile: legacy },
    { ...input, profile: { ...profile, memory: { ...profile.memory!, manifestId: 'foreign' } } },
  ])
    await expect(solveMemory(bad, provider, open)).rejects.toThrow();
  expect(open).not.toHaveBeenCalled();
  expect(run).not.toHaveBeenCalled();
});
it('freezes profile and task before asynchronous admission without adding arbitrary host fields to the model input', async () => {
  const selected = structuredClone(input);
  Object.assign(selected.profile, { secret: 'HOST_FIELD_CANARY' });
  open.mockImplementationOnce(() => {
    Object.assign(selected.profile, { label: 'MUTATED_LABEL', archetype: 'MUTATED_ARCHETYPE' });
    selected.task = 'MUTATED_TASK';
    return Promise.resolve(memory);
  });
  await solveMemory(selected, provider, open);
  expect(run.mock.calls[0]![0].task).not.toMatch(/MUTATED|HOST_FIELD_CANARY/);
  expect(run.mock.calls[0]![0].task).toContain(JSON.stringify(input.task));
});
it.each(['open', 'reserve', 'model', 'close'])(
  'discards responses cancelled during %s and closes acquired memory',
  async (phase) => {
    const controller = new AbortController();
    const stop = () => controller.abort(new Error('late cancellation'));
    if (phase === 'open')
      open.mockImplementationOnce(() => {
        stop();
        return Promise.resolve(memory);
      });
    if (phase === 'reserve')
      reserve.mockImplementationOnce((draft) => {
        stop();
        return Promise.resolve(lease(draft, memory.session));
      });
    if (phase === 'model')
      run.mockImplementationOnce(() => {
        stop();
        return Promise.resolve({ answer: JSON.stringify(reply), trace });
      });
    if (phase === 'close')
      close.mockImplementationOnce(() => {
        stop();
        return Promise.resolve();
      });
    await expect(solveMemory(input, provider, open, controller.signal)).rejects.toThrow(
      'late cancellation',
    );
    expect(close).toHaveBeenCalledOnce();
    if (phase === 'open' || phase === 'reserve') expect(run).not.toHaveBeenCalled();
  },
);
it('keeps both operation and cleanup failures without reporting a successful answer', async () => {
  const failure = new Error('CLI failed');
  const cleanup = new Error('close failed');
  run.mockRejectedValueOnce(failure);
  close.mockRejectedValueOnce(cleanup);
  const result = await solveMemory(input, provider, open).catch((error: unknown) => error);
  expect(result).toBeInstanceOf(AggregateError);
  expect((result as AggregateError).errors).toEqual([failure, cleanup]);
});
it('enforces the native deadline and removes its timer after cancellation and cleanup', async () => {
  vi.useFakeTimers();
  try {
    run.mockImplementationOnce(
      (_input, signal) =>
        new Promise((resolve) => {
          signal!.addEventListener(
            'abort',
            () => resolve({ answer: JSON.stringify(reply), trace }),
            { once: true },
          );
        }),
    );
    const result = solveMemory(input, provider, open).catch((error: unknown) => error);
    await vi.advanceTimersByTimeAsync(60000);
    expect(await result).toEqual(new Error('Истёк срок ответа Person.'));
    expect(close).toHaveBeenCalledOnce();
    expect(vi.getTimerCount()).toBe(0);
  } finally {
    vi.useRealTimers();
  }
});
it('reads only an observed episode and preserves original words, author and relationships in the final native evidence', async () => {
  const found = sourceRetrieval();
  const episode = sourceRetrieval('receipt:episode', 'readEpisode');
  let calls = 0;
  reserve.mockImplementation((draft) =>
    Promise.resolve(lease(draft, memory.session, ++calls, [found, episode])),
  );
  recall.mockResolvedValueOnce(found);
  readEpisode.mockResolvedValueOnce({
    episodeId: 'episode:1',
    receipt: episode.receipt,
    sources: episode.sources,
  });
  run.mockResolvedValueOnce({
    answer: JSON.stringify({ action: 'recall', query: 'Борис', intent: 'context' }),
    trace,
  });
  run.mockResolvedValueOnce({
    answer: JSON.stringify({ action: 'readEpisode', episodeId: 'episode:1' }),
    trace,
  });
  const result = await solveMemory(input, provider, open);
  expect(readEpisode).toHaveBeenCalledExactlyOnceWith('episode:1', expect.any(AbortSignal));
  expect(result.retrievals).toEqual([
    { action: { action: 'recall', query: 'Борис', intent: 'context' }, receipt: found.receipt },
    { action: { action: 'readEpisode', episodeId: 'episode:1' }, receipt: episode.receipt },
  ]);
  const observed = JSON.stringify(result.retrievals);
  expect(observed).not.toContain(found.sources[0]!.text);
  Object.assign(found.receipt, { id: 'mutated-after-return' });
  expect(JSON.stringify(result.retrievals)).toBe(observed);
  expect(result.context.sources).toEqual([...found.sources, ...episode.sources]);
  expect(run.mock.calls[2]![0].task).toContain(JSON.stringify(found.sources[0]!.text));
  expect(result.context.items).toEqual([]);
  expect(result.reply?.messages).toEqual(messages);
});
it('rejects invalid Unicode actions before the memory port and repairs them into a reply', async () => {
  run.mockResolvedValueOnce({
    answer: JSON.stringify({ action: 'recall', query: '\ud800', intent: 'facts' }),
    trace,
  });
  await solveMemory(input, provider, open);
  expect(recall).not.toHaveBeenCalled();
  expect(reserve.mock.calls.map(([draft]) => draft.formatAttempt)).toEqual([1, 2]);
});
it('selects profile evidence once and keeps proposed procedural skills and their limitations explicit', async () => {
  const base = sourceRetrieval('receipt:baseline', 'readEpisode');
  const source = base.sources[0]!;
  const item: MemoryItem = {
    schemaVersion: 1,
    id: 'item:1',
    snapshotId: 'corpus:1',
    kind: 'procedure',
    text: source.text,
    sourceSpans: [source.span],
    episodeIds: ['episode:1'],
    eventAt: null,
    observedAt: '2026-11-09T00:00:00Z',
    support: 'reported',
    contradictionIds: [],
    limitations: ['Сообщение автора'],
    extractionVersion: 'stalker-memory:1',
    policyVersion: 'policy:1',
  };
  const skill: SkillDefinition = {
    schemaVersion: 1,
    id: 'skill:1',
    trigger: 'При проверке',
    inputs: [],
    steps: ['  Проверить\r\nграницы'],
    outcomeCheck: 'Основания указаны',
    support: 'proposed',
    memoryItemIds: [item.id],
    sourceSpans: [source.span],
    limitations: ['Предполагаемый, не продемонстрированный навык'],
  };
  const chosen = {
    ...profile,
    rules: [
      {
        id: 'rule:1',
        instruction: 'Проверить границы',
        evidenceIds: [item.id],
        status: 'hypothesis' as const,
        limitations: ['Одна запись'],
      },
    ],
    memory: { ...profile.memory!, skills: [skill] },
  };
  memory = {
    ...memory,
    session: {
      ...memory.session,
      skills: [skill],
      manifest: {
        ...memory.session.manifest,
        skillIds: [skill.id],
        profileSnapshot: snapshotOf(chosen),
      },
    },
  };
  reserve.mockImplementation((draft) =>
    Promise.resolve(lease(draft, memory.session, 1, [], [item], [base])),
  );
  const result = await solveMemory({ ...input, profile: chosen }, provider, open);
  expect(reserve.mock.calls[0]![0].memoryItemIds).toEqual([item.id]);
  expect(JSON.parse(reserve.mock.calls[0]![0].task)).toHaveProperty('proceduralSkills', [skill]);
  expect(result.context.items).toEqual([item]);
  expect(result.context.sources).toEqual(base.sources);
});

it('saves the exact successful CLI output before closing, excluding repair attempts and host scope from prompt', async () => {
  const raw = '\uFEFF```json\n' + JSON.stringify(reply, null, 2) + '\n``` \r\n';
  run.mockResolvedValueOnce({ invocationId: 'bad-call', answer: 'not JSON', trace });
  run.mockResolvedValueOnce({ invocationId: 'final-call', answer: raw, trace });
  const result = await solveMemory(input, provider, open);
  expect(append).toHaveBeenCalledExactlyOnceWith(
    {
      schemaVersion: 1,
      ...input.simulation,
      contextLeaseId: 'lease:2',
      invocationId: 'final-call',
      answer: raw,
    },
    expect.any(AbortSignal),
  );
  expect(append.mock.invocationCallOrder[0]).toBeLessThan(close.mock.invocationCallOrder[0]!);
  expect(result).toHaveProperty('simulation.id', 'simulation:abc');
  expect(run.mock.calls[0]![0].task).not.toContain(input.simulation.experimentId);
});
it('repairs aggregate oversized dialogue before native publication', async () => {
  run.mockResolvedValueOnce({
    invocationId: 'long-call',
    answer: JSON.stringify({ ...reply, messages: ['a'.repeat(40000), 'b'.repeat(40000)] }),
    trace,
  });
  await solveMemory(input, provider, open);
  expect(run).toHaveBeenCalledTimes(2);
  expect(append).toHaveBeenCalledOnce();
  expect(append.mock.calls[0]![0].contextLeaseId).toBe('lease:2');
});
it.each(['missingInvocation', 'foreignText', 'saveFailure', 'cancelled'])(
  'does not report success after invalid or failed publication: %s',
  async (kind) => {
    const cancel = new AbortController();
    if (kind === 'missingInvocation')
      run.mockResolvedValueOnce({ answer: JSON.stringify(reply), trace });
    if (kind === 'foreignText')
      append.mockResolvedValueOnce({
        schemaVersion: 1,
        id: 'simulation:abc',
        ...input.simulation,
        manifestId: request.manifestId,
        role: 'simulation',
        text: 'forged',
        createdAt: '2026-11-09T00:00:00.000Z',
      });
    if (kind === 'saveFailure') append.mockRejectedValueOnce(new Error('revoked'));
    if (kind === 'cancelled')
      append.mockImplementationOnce(() => {
        cancel.abort(new Error('late save'));
        return Promise.reject(new Error('late save'));
      });
    await expect(solveMemory(input, provider, open, cancel.signal)).rejects.toThrow();
    expect(close).toHaveBeenCalledOnce();
    if (kind === 'missingInvocation') expect(append).not.toHaveBeenCalled();
  },
);
it('rejects malformed host simulation scope before opening and freezes it across admission', async () => {
  await expect(
    solveMemory(
      { ...input, simulation: { ...input.simulation, trialId: 'bad\n' } },
      provider,
      open,
    ),
  ).rejects.toThrow();
  expect(open).not.toHaveBeenCalled();
  const selected = structuredClone(input);
  open.mockImplementationOnce(() => {
    selected.simulation.trialId = 'mutated';
    return Promise.resolve(memory);
  });
  await solveMemory(selected, provider, open);
  expect(append.mock.calls[0]![0].trialId).toBe(input.simulation.trialId);
});

it('repairs null conflict metadata rather than deferring an invalid reply to native storage', async () => {
  run.mockResolvedValueOnce({
    invocationId: 'bad-call',
    answer: JSON.stringify({ ...reply, styleConflicts: null }),
    trace,
  });
  await solveMemory(input, provider, open);
  expect(run).toHaveBeenCalledTimes(2);
  expect(append.mock.calls[0]![0].contextLeaseId).toBe('lease:2');
});

it('rejects changed profile text under the same native manifest before invoking the model', async () => {
  for (const changed of [
    { ...profile, label: 'changed label' },
    { ...profile, archetype: 'changed archetype' },
    { ...profile, skills: ['changed general skill'] },
    {
      ...profile,
      taskSkills: [
        {
          id: 's1',
          instruction: 'changed task rule',
          evidenceIds: [],
          status: 'hypothesis' as const,
          limitations: [],
        },
      ],
    },
  ]) {
    await expect(solveMemory({ ...input, profile: changed }, provider, open)).rejects.toThrow();
  }
  expect(run).not.toHaveBeenCalled();
  expect(reserve).not.toHaveBeenCalled();
});

it('rejects same-ID procedural, rule and verbatim-example substitutions before native reservation', async () => {
  const skill: SkillDefinition = {
    schemaVersion: 1,
    id: 'skill:bound',
    trigger: 'Проверка',
    inputs: [],
    steps: ['  Исходный шаг é\r\n'],
    outcomeCheck: 'Сверено',
    support: 'proposed',
    memoryItemIds: ['item:1'],
    sourceSpans: [
      { documentId: 'doc:1', revisionId: 'rev:1', unit: 'utf8Bytes', start: 0, end: 5 },
    ],
    limitations: ['Предложение'],
  };
  const chosen = {
    ...profile,
    rules: [
      {
        id: 'r1',
        instruction: '  Правило\r\n',
        evidenceIds: ['item:1'],
        status: 'hypothesis' as const,
        limitations: [],
      },
    ],
    memory: { ...profile.memory!, skills: [skill] },
    styleExemplars: [
      {
        id: 'sample:1',
        documentId: 'doc:1',
        mode: 'dialogue' as const,
        text: '  Дословно 👩🏽‍🔬\r\n',
        context: 'Контекст',
        redactions: [],
        exportAllowed: false,
      },
    ],
    styleProfile: {
      ...profile.styleProfile!,
      voiceRules: [
        {
          id: 'voice-1',
          instruction: 'Не менять слова',
          exemplarIds: ['sample:1'],
          status: 'observed' as const,
          limitations: [],
        },
      ],
      styleExemplarIds: ['sample:1'],
      contextModes: profile.styleProfile!.contextModes.map((mode) => ({
        ...mode,
        status: mode.mode === 'dialogue' ? ('observed' as const) : ('insufficient' as const),
      })),
    },
  };
  memory = {
    ...memory,
    session: {
      ...memory.session,
      skills: [skill],
      manifest: {
        ...memory.session.manifest,
        skillIds: [skill.id],
        exemplarIds: ['sample:1'],
        profileSnapshot: snapshotOf(chosen),
      },
    },
  };
  for (const changed of [
    { ...chosen, memory: { ...chosen.memory, skills: [{ ...skill, steps: ['changed step'] }] } },
    { ...chosen, rules: [{ ...chosen.rules[0]!, instruction: 'changed rule' }] },
    { ...chosen, styleExemplars: [{ ...chosen.styleExemplars[0]!, text: 'changed words' }] },
    {
      ...chosen,
      styleProfile: {
        ...chosen.styleProfile,
        voiceRules: [{ ...chosen.styleProfile.voiceRules[0]!, instruction: 'changed voice' }],
      },
    },
  ])
    await expect(solveMemory({ ...input, profile: changed }, provider, open)).rejects.toThrow();
  expect(run).not.toHaveBeenCalled();
  expect(reserve).not.toHaveBeenCalled();
});

it('passes precise repair feedback as data under the fixed persona instructions', async () => {
  run.mockResolvedValueOnce({ invocationId: 'bad-call', answer: 'not JSON', trace });
  await solveMemory(input, provider, open);
  const second = reserve.mock.calls[1]![0];
  expect(JSON.parse(second.task)).toHaveProperty('formatFeedback', 'Invalid JSON syntax');
  expect(second.systemPrompt).not.toContain('failed validation: Invalid JSON syntax');
  expect(second.systemPrompt).toContain('formatFeedback');
});
