import { beforeEach, expect, it, vi } from 'vitest';
import type {
  FabricMemoryPort,
  ManifestFreezeReceipt,
  PersonaManifestDraft,
  SkillDefinition,
} from '@proteus-int/contracts';
import type { IntelligenceProvider } from '@proteus-int/intelligence';
import { fabricateMemory } from './memory-fabric';
import { adaptMemory } from './adapt-memory';
import type { MemoryFabricInput } from './memory-fabric';
import { request, selection, session, source, lease } from './testing/fabric-fixtures';
import { skillMemoryItem } from './testing/memory-skill-fixtures';

const items = [skillMemoryItem(), skillMemoryItem({ id: 'item-2', kind: 'styleExample' })];
const input: MemoryFabricInput = {
  person: {
    label: 'Анна',
    context: 'Анна пишет Борису\r\n@boris',
    publicUrls: ['https://example.org/ann'],
  },
  palette: 'turquoise',
  request,
  ...selection,
  itemIds: items.map((item) => item.id),
  maxSkills: 10,
  manifest: {
    mode: 'agenticRecall',
    recallBudget: request.recallBudget,
    modelCallBudget: request.modelBudget,
    modelSettings: (['stalker', 'person-fabric', 'person', 'brain-analyzator'] as const).map(
      (component) => ({
        component,
        provider: request.model.provider,
        requestedModel: request.model.model,
        requestedEffort: request.model.effort,
      }),
    ),
  },
};
const output = {
  rules: [
    {
      id: 'r1',
      instruction: 'Сначала проверяю границы',
      evidenceIds: ['item-1'],
      status: 'supported',
      limitations: ['Одна запись'],
    },
  ],
  style: {
    reasoningPatterns: [
      {
        id: 'reason-1',
        instruction: 'Проверяет границы',
        exemplarIds: ['item-2'],
        status: 'observed',
        limitations: [],
      },
    ],
    voiceRules: [],
    formatRules: [],
    limitations: [],
  },
  skills: [
    {
      draftId: 'draft:1',
      trigger: '  Перед началом 👩🏽‍🔬\r\n',
      inputs: [],
      steps: ['  Сверить границы é\r\n'],
      outcomeCheck: 'Границы проверены',
      support: 'observed',
      memoryItemIds: ['item-1'],
      sourceSpans: items[0]!.sourceSpans,
      limitations: [],
    },
  ],
};
const trace = { provider: 'codex' as const, reasoningMs: 1, terminalMs: 2, summary: 'done' };
const run = vi.fn<IntelligenceProvider['run']>();
const reserve = vi.fn<FabricMemoryPort['reserveModelContext']>();
const freeze = vi.fn<FabricMemoryPort['freezeManifest']>();
const close = vi.fn<FabricMemoryPort['close']>();
const provider: IntelligenceProvider = { id: 'codex', run };
const memory: FabricMemoryPort = {
  session,
  reserveModelContext: reserve,
  freezeManifest: freeze,
  close,
};
const open = vi.fn(() => Promise.resolve(memory));
function publication(draft: PersonaManifestDraft): ManifestFreezeReceipt {
  const skills: SkillDefinition[] = draft.skills.map((skill, index) => ({
    schemaVersion: 1,
    id: `memorySkill:${index}`,
    trigger: skill.trigger,
    inputs: skill.inputs,
    steps: skill.steps,
    outcomeCheck: skill.outcomeCheck,
    support: skill.support,
    memoryItemIds: skill.memoryItemIds,
    sourceSpans: skill.sourceSpans,
    limitations: skill.limitations,
  }));
  return {
    schemaVersion: 1,
    skills,
    manifest: {
      ...(draft.schemaVersion === 1
        ? { schemaVersion: 1 as const }
        : { schemaVersion: 2 as const, profileSnapshot: structuredClone(draft.profileSnapshot) }),
      id: 'personaManifest:1',
      personaId: session.personaId,
      profileVersion: draft.profileVersion,
      promptVersion: draft.promptVersion,
      skillIds: skills.map((skill) => skill.id),
      exemplarIds: draft.exemplarIds,
      corpusSnapshotId: session.corpusSnapshotId,
      memorySnapshotId: draft.memorySnapshotId,
      sourceRefs: [{ documentId: source.span.documentId, revisionId: source.span.revisionId }],
      splitVersion: 'split:1',
      preparationVersion: 'nativePreparation:1:' + 'a'.repeat(64),
      retrievalVersion: 'nativeMemoryRetrieval:1',
      embeddingFingerprint: null,
      mode: draft.mode,
      recallBudget: draft.recallBudget,
      modelCallBudget: draft.modelCallBudget,
      modelSettings: draft.modelSettings,
      createdAt: '2026-11-09T03:00:01.004Z',
    },
  };
}
beforeEach(() => {
  vi.resetAllMocks();
  open.mockResolvedValue(memory);
  let calls = 0;
  reserve.mockImplementation((draft) => Promise.resolve(lease(draft, ++calls, items)));
  run.mockResolvedValue({ answer: JSON.stringify(output), trace });
  freeze.mockImplementation((draft) => Promise.resolve(publication(draft)));
  close.mockResolvedValue();
});

it('adapts a published base through a fresh native Fabric context without publishing a new persona', async () => {
  const prepared = await fabricateMemory(input, provider, open);
  const original = structuredClone(prepared.publication);
  const task = '\uFEFFНапиши Борису é 👩🏽‍🔬\r\n';
  run.mockResolvedValue({
    answer: JSON.stringify({
      skills: [
        {
          id: 's1',
          instruction: '  Сверь условия\r\n',
          evidenceIds: ['item-1'],
          status: 'supported',
          limitations: [],
        },
      ],
    }),
    trace,
  });
  const result = await adaptMemory(
    { publication: prepared.publication, request, task },
    provider,
    open,
  );
  expect(result.adaptation).toEqual({
    schemaVersion: 1,
    manifestId: prepared.publication.manifest.id,
    task,
    strategies: [
      {
        id: 's1',
        instruction: 'Сверь условия',
        evidenceIds: ['item-1'],
        status: 'hypothesis',
        limitations: ['Одна запись'],
      },
    ],
  });
  expect(result.traces).toEqual([trace]);
  expect(prepared.publication).toEqual(original);
  expect(prepared.profile.taskSkills).toEqual([]);
  expect(freeze).toHaveBeenCalledOnce();
  expect(open).toHaveBeenCalledTimes(2);
  expect(close).toHaveBeenCalledTimes(2);
  const sent = run.mock.calls[1]![0];
  expect(sent).toMatchObject({
    operation: 'generation',
    sourceContext: true,
    memoryContext: { sessionId: session.id, leaseId: 'lease:2' },
    model: request.model.model,
    effort: request.model.effort,
  });
  expect(sent.task).toContain(JSON.stringify(task));
  expect(reserve.mock.calls[1]![0]).toMatchObject({
    memorySnapshotId: prepared.publication.manifest.memorySnapshotId,
    itemIds: ['item-1', 'item-2'],
  });
});

it('rejects mismatched adaptation scope and malformed task before opening native memory', async () => {
  const prepared = await fabricateMemory(input, provider, open);
  open.mockClear();
  run.mockClear();
  for (const candidate of [
    {
      publication: prepared.publication,
      request: { ...request, personaId: 'foreign' },
      task: 'task',
    },
    {
      publication: prepared.publication,
      request: { ...request, snapshotId: 'foreign' },
      task: 'task',
    },
    {
      publication: prepared.publication,
      request: { ...request, model: { ...request.model, model: 'foreign' } },
      task: 'task',
    },
    { publication: prepared.publication, request, task: '\ud800' },
  ])
    await expect(adaptMemory(candidate, provider, open)).rejects.toThrow();
  expect(open).not.toHaveBeenCalled();
  expect(run).not.toHaveBeenCalled();
});

it('rechecks source permission before sending a task adaptation to the provider and closes on refusal', async () => {
  const prepared = await fabricateMemory(input, provider, open);
  run.mockClear();
  close.mockClear();
  reserve.mockRejectedValueOnce(new Error('grant revoked'));
  await expect(
    adaptMemory({ publication: prepared.publication, request, task: 'task' }, provider, open),
  ).rejects.toThrow('grant revoked');
  expect(run).not.toHaveBeenCalled();
  expect(close).toHaveBeenCalledOnce();
  expect(freeze).toHaveBeenCalledOnce();
});

it('repairs invented adaptation evidence with a fresh lease and retains both observable traces', async () => {
  const prepared = await fabricateMemory(input, provider, open);
  const strategy = {
    id: 's1',
    instruction: 'Уточнить условия',
    evidenceIds: ['item-1'],
    status: 'hypothesis',
    limitations: [],
  };
  run.mockResolvedValueOnce({
    answer: JSON.stringify({ skills: [{ ...strategy, evidenceIds: ['foreign'] }] }),
    trace,
  });
  run.mockResolvedValueOnce({ answer: JSON.stringify({ skills: [strategy] }), trace });
  const result = await adaptMemory(
    { publication: prepared.publication, request, task: 'task' },
    provider,
    open,
  );
  expect(reserve.mock.calls.slice(1).map(([draft]) => draft.formatAttempt)).toEqual([1, 2]);
  expect(result.traces).toEqual([trace, trace]);
  expect(result.adaptation.strategies[0]?.evidenceIds).toEqual(['item-1']);
  expect(freeze).toHaveBeenCalledOnce();
});

it('snapshots the stored profile, task and model before awaiting the adaptation session', async () => {
  const prepared = await fabricateMemory(input, provider, open);
  const chosen = {
    publication: structuredClone(prepared.publication),
    request: structuredClone(request),
    task: '  Exact task\r\n',
  };
  const expectedTask = chosen.task;
  open.mockImplementationOnce(() => {
    chosen.task = 'changed task';
    Object.assign(chosen.request.model, { model: 'changed model' });
    if (chosen.publication.manifest.schemaVersion === 2)
      Object.assign(chosen.publication.manifest.profileSnapshot, {
        archetype: 'CHANGED_PROFILE_CANARY',
      });
    return Promise.resolve(memory);
  });
  run.mockResolvedValue({
    answer: JSON.stringify({
      skills: [
        {
          id: 's1',
          instruction: 'Уточнить условия',
          evidenceIds: [],
          status: 'hypothesis',
          limitations: [],
        },
      ],
    }),
    trace,
  });
  const result = await adaptMemory(chosen, provider, open);
  const sent = run.mock.calls[1]![0];
  expect(sent.task).toContain(JSON.stringify(expectedTask));
  expect(sent.task).not.toContain('CHANGED_PROFILE_CANARY');
  expect(sent.model).toBe(request.model.model);
  expect(result.adaptation.task).toBe(expectedTask);
});
it('builds a task-free profile with observed skills, exact exemplars and a native frozen manifest', async () => {
  const result = await fabricateMemory(
    {
      ...input,
      task: 'FUTURE_TASK_CANARY',
      person: { ...input.person, task: 'NESTED_TASK_CANARY' },
    } as MemoryFabricInput,
    provider,
    open,
  );
  const sent = run.mock.calls[0]![0];
  expect(sent.task).not.toContain('TASK_CANARY');
  expect(sent.task).toContain(JSON.stringify(input.person));
  expect(sent.systemPrompt).toContain('without anticipating');
  expect(sent.operation).toBe('generation');
  expect(freeze.mock.calls[0]![0]).toMatchObject({
    contextLeaseIds: ['lease:1'],
    exemplarIds: ['item-2'],
    skills: output.skills,
  });
  expect(result.profile).toMatchObject({
    id: session.personaId,
    label: 'Анна',
    palette: 'turquoise',
    rules: output.rules,
    taskSkills: [],
    skills: [],
    sourceStatus: 'evidence-informed',
    sourceContext: true,
    memory: {
      schemaVersion: 1,
      manifestId: 'personaManifest:1',
      memorySnapshotId: selection.memorySnapshotId,
      skills: result.publication.skills,
    },
  });
  expect(result.profile.styleExemplars?.[0]).toMatchObject({
    id: 'item-2',
    text: items[1]!.text,
    mode: 'dialogue',
    redactions: [],
    exportAllowed: false,
  });
  expect(result.profile.styleExemplars?.[0]?.context).toContain('Анна');
  expect(result.profile.styleProfile?.reasoningPatterns[0]?.status).toBe('hypothesis');
  expect(result.traces).toEqual([trace]);
  expect(close).toHaveBeenCalledOnce();
});
it('repairs unsupported skills before freeze and never upgrades suggested skills into observations', async () => {
  run.mockResolvedValueOnce({
    answer: JSON.stringify({
      ...output,
      skills: [{ ...output.skills[0], memoryItemIds: ['item-2'] }],
    }),
    trace,
  });
  const result = await fabricateMemory(input, provider, open);
  expect(reserve.mock.calls.map(([draft]) => draft.formatAttempt)).toEqual([1, 2]);
  expect(freeze.mock.calls[0]![0].contextLeaseIds).toEqual(['lease:1', 'lease:2']);
  expect(result.traces).toHaveLength(2);
  expect(result.profile.memory?.skills[0]?.steps).toEqual(output.skills[0]!.steps);
});
it('rejects invalid host scope and mismatched Fabric model settings before opening a session', async () => {
  for (const value of [
    { ...input, itemIds: ['x', 'x'] },
    { ...input, maxSkills: 11 },
    { ...input, palette: 'orange' },
    { ...input, person: { ...input.person, label: '\ud800' } },
    {
      ...input,
      manifest: {
        ...input.manifest,
        modelSettings: input.manifest.modelSettings.map((model) =>
          model.component === 'person-fabric' ? { ...model, requestedModel: 'foreign' } : model,
        ),
      },
    },
  ])
    await expect(fabricateMemory(value as MemoryFabricInput, provider, open)).rejects.toThrow();
  expect(open).not.toHaveBeenCalled();
});
it('closes after invalid output or a failed freeze without exposing a completed profile', async () => {
  run.mockResolvedValue({
    answer: JSON.stringify({
      ...output,
      rules: [{ ...output.rules[0], evidenceIds: ['foreign'] }],
    }),
    trace,
  });
  await expect(fabricateMemory(input, provider, open)).rejects.toThrow();
  expect(freeze).not.toHaveBeenCalled();
  expect(close).toHaveBeenCalledOnce();
  run.mockResolvedValue({ answer: JSON.stringify(output), trace });
  freeze.mockRejectedValueOnce(new Error('grant revoked'));
  await expect(fabricateMemory(input, provider, open)).rejects.toThrow('grant revoked');
  expect(close).toHaveBeenCalledTimes(2);
});
it('discards a cancelled freeze response and still closes the native session', async () => {
  const controller = new AbortController();
  freeze.mockImplementationOnce((draft) => {
    controller.abort(new Error('stop freeze'));
    return Promise.resolve(publication(draft));
  });
  await expect(fabricateMemory(input, provider, open, controller.signal)).rejects.toThrow(
    'stop freeze',
  );
  expect(close).toHaveBeenCalledOnce();
});
it('allows an explicitly empty selected memory without inventing traits, samples or skills', async () => {
  reserve.mockImplementation((draft) => Promise.resolve(lease(draft, 1, [])));
  run.mockResolvedValue({
    answer: JSON.stringify({
      rules: [],
      skills: [],
      style: {
        reasoningPatterns: [],
        voiceRules: [],
        formatRules: [],
        limitations: ['Нет образцов'],
      },
    }),
    trace,
  });
  freeze.mockImplementation((draft) => {
    const result = publication(draft);
    return Promise.resolve({ ...result, manifest: { ...result.manifest, sourceRefs: [] } });
  });
  const result = await fabricateMemory({ ...input, itemIds: [] }, provider, open);
  expect(result.profile).toMatchObject({
    sourceStatus: 'synthetic',
    rules: [],
    styleExemplars: [],
    memory: { skills: [] },
  });
  expect(result.profile.archetype).toContain('Недостаточно сведений');
  expect(freeze.mock.calls[0]![0].exemplarIds).toEqual([]);
});
it('rejects a substituted native manifest and surfaces both preparation and cleanup errors', async () => {
  freeze.mockImplementationOnce((draft) => {
    const result = publication(draft);
    return Promise.resolve({ ...result, manifest: { ...result.manifest, personaId: 'foreign' } });
  });
  await expect(fabricateMemory(input, provider, open)).rejects.toThrow();
  expect(close).toHaveBeenCalledOnce();
  const failure = new Error('permission revoked');
  const cleanup = new Error('close transport failed');
  freeze.mockRejectedValueOnce(failure);
  close.mockRejectedValueOnce(cleanup);
  const result = await fabricateMemory(input, provider, open).catch((error: unknown) => error);
  expect(result).toBeInstanceOf(AggregateError);
  expect((result as AggregateError).errors).toEqual([failure, cleanup]);
});
it('does not return a profile cancelled while native cleanup finishes', async () => {
  const controller = new AbortController();
  close.mockImplementationOnce(() => {
    controller.abort(new Error('cancel during close'));
    return Promise.resolve();
  });
  await expect(fabricateMemory(input, provider, open, controller.signal)).rejects.toThrow(
    'cancel during close',
  );
});
it('enforces the overall preparation deadline and does not publish a late model answer', async () => {
  vi.useFakeTimers();
  try {
    run.mockImplementationOnce(
      (_input, signal) =>
        new Promise((resolve) => {
          signal!.addEventListener(
            'abort',
            () => resolve({ answer: JSON.stringify(output), trace }),
            { once: true },
          );
        }),
    );
    const result = fabricateMemory(input, provider, open).catch((error: unknown) => error);
    await vi.advanceTimersByTimeAsync(60000);
    expect(await result).toEqual(new Error('Истёк срок подготовки Fabric.'));
    expect(freeze).not.toHaveBeenCalled();
    expect(close).toHaveBeenCalledOnce();
    expect(vi.getTimerCount()).toBe(0);
  } finally {
    vi.useRealTimers();
  }
});

it('freezes the entire task-free body before returning a profile', async () => {
  const result = await fabricateMemory(input, provider, open);
  expect(freeze.mock.calls[0]![0]).toMatchObject({
    schemaVersion: 2,
    profileVersion: 'person-memory-profile:2',
    promptVersion: 'person-fabric-memory:2',
    profileSnapshot: {
      schemaVersion: 1,
      label: result.profile.label,
      palette: result.profile.palette,
      archetype: result.profile.archetype,
      rules: result.profile.rules,
      sourceStatus: result.profile.sourceStatus,
      styleProfile: result.profile.styleProfile,
      styleExemplars: result.profile.styleExemplars,
    },
  });
  expect(result.publication.manifest.schemaVersion).toBe(2);
});
