import { expect, it } from 'vitest';
import type { OpenFabricMemory } from '@proteus-int/person-fabric';
import type { OpenResearchMemory } from '@proteus-int/stalker';
import { prepareMemoryPersona } from './prepare-memory-persona';
import type { PrepareMemoryPersonaInput } from './prepare-memory-persona';
import {
  preparationEpisodeText,
  preparationFabricTrace,
  preparationHosts,
  preparationInput,
  preparationResearchTrace,
} from './preparation-fixtures';

const budgetLimitation =
  'Поиск дополнительных оснований ограничен бюджетом; попытки сохранены для извлечения памяти.';

it('preserves an explicit neutral role through research and native profile freeze', async () => {
  const input = { ...preparationInput(), role: 'neutral' as const };
  const hosts = preparationHosts(input, {
    fabricProfileAnswer: {
      rules: [],
      style: { reasoningPatterns: [], voiceRules: [], formatRules: [], limitations: [] },
      skills: [],
    },
  });
  const result = await prepareMemoryPersona(
    input,
    hosts.researchProvider,
    hosts.fabricProvider,
    hosts.openResearch,
    hosts.openFabric,
  );
  expect(result.profile.sourceStatus).toBe('neutral');
  expect(result.profile.styleExemplars).toEqual([]);
  expect(result.freeze.manifest).toMatchObject({
    schemaVersion: 2,
    profileSnapshot: { sourceStatus: 'neutral' },
    exemplarIds: [],
  });
  expect(hosts.researchModelRuns[0]!.task).toContain(JSON.stringify(input.person.label));
});

it('researches memory first, then fabricates one task-free persona from the native publication', async () => {
  const input = preparationInput();
  const hosts = preparationHosts(input);
  const result = await prepareMemoryPersona(
    input,
    hosts.researchProvider,
    hosts.fabricProvider,
    hosts.openResearch,
    hosts.openFabric,
  );
  expect(hosts.events).toEqual([
    'research:open',
    'research:read:episode:1',
    'research:read:episode:2',
    'research:reserve',
    'research:model',
    'research:publish',
    'research:close',
    'fabric:open',
    'fabric:reserve',
    'fabric:model',
    'fabric:freeze',
    'fabric:close',
  ]);
  // Exact selected native scope and settings reach each stage; budgets may differ.
  expect(hosts.openResearchRequests).toEqual([input.research]);
  expect(hosts.openFabricRequests).toEqual([input.fabric]);
  expect(hosts.openResearchRequests[0]!.recallBudget.maxCalls).toBe(10);
  expect(hosts.openFabricRequests[0]!.recallBudget.maxCalls).toBe(20);
  expect(hosts.openFabricRequests[0]!.model).toEqual({
    provider: 'codex',
    model: 'fabric-model',
    effort: 'high',
  });
  // The original research publication is returned untouched, not regenerated.
  expect(result.publication).toBe(hosts.researchReceipts[0]);
  expect(result.publication.snapshot.id).toBe('memory:preparation-1');
  expect(result.researchLimitations).toEqual([budgetLimitation]);
  expect(result.researchTraces).toEqual([preparationResearchTrace]);
  expect(result.fabricTraces).toEqual([preparationFabricTrace]);
  // The full Fabric profile and freeze receipt come back with exact strings.
  expect(result.profile).toMatchObject({
    id: 'persona:preparation-1',
    label: 'Анна',
    palette: 'turquoise',
    sourceStatus: 'evidence-informed',
    rules: [
      {
        id: 'r1',
        instruction: 'Сначала проверяю границы',
        evidenceIds: ['native:1'],
        status: 'supported',
      },
    ],
    memory: {
      schemaVersion: 1,
      manifestId: 'personaManifest:preparation-1',
      memorySnapshotId: 'memory:preparation-1',
    },
  });
  expect(result.profile.styleExemplars?.[0]).toMatchObject({
    id: 'native:2',
    text: preparationEpisodeText.second,
    exportAllowed: false,
  });
  expect(result.profile.memory?.skills[0]).toMatchObject({
    id: 'memorySkill:1',
    trigger: 'Перед началом работы',
    steps: ['Сверить границы'],
    support: 'observed',
  });
  expect(result.freeze.manifest).toMatchObject({
    schemaVersion: 2,
    id: 'personaManifest:preparation-1',
    memorySnapshotId: 'memory:preparation-1',
    exemplarIds: ['native:2'],
  });
  expect(
    result.freeze.manifest.modelSettings.find((entry) => entry.component === 'person-fabric'),
  ).toMatchObject({ provider: 'codex', requestedModel: 'fabric-model', requestedEffort: 'high' });
  // Exactly one bounded model call per stage: no scoring, answering or extra work.
  expect(hosts.researchModelRuns).toHaveLength(1);
  expect(hosts.fabricModelRuns).toHaveLength(1);
  expect(hosts.researchModelRuns[0]!.operation).toBe('corpus-analysis');
  expect(hosts.fabricModelRuns[0]!.operation).toBe('generation');
  for (const run of [...hosts.researchModelRuns, ...hosts.fabricModelRuns]) {
    expect(run.sourceContext).toBe(true);
    expect(run.operation).not.toBe('persona-reply');
    expect(run.task).toContain(JSON.stringify(preparationEpisodeText.second).slice(1, -1));
  }
});

it('lets the native publication snapshot id and ordered published item ids drive Fabric', async () => {
  const input = preparationInput();
  const hosts = preparationHosts(input, {
    publicationSnapshotId: 'memory:native-chosen-9',
    publicationItemIds: ['native:9', 'native:10'],
  });
  const result = await prepareMemoryPersona(
    input,
    hosts.researchProvider,
    hosts.fabricProvider,
    hosts.openResearch,
    hosts.openFabric,
  );
  expect(hosts.fabricReservations[0]).toMatchObject({
    memorySnapshotId: 'memory:native-chosen-9',
    itemIds: ['native:9', 'native:10'],
  });
  expect(result.freeze.manifest.memorySnapshotId).toBe('memory:native-chosen-9');
  expect(result.freeze.manifest.exemplarIds).toEqual(['native:10']);
  expect(result.profile.rules[0]?.evidenceIds).toEqual(['native:9']);
  expect(result.profile.styleExemplars?.[0]?.id).toBe('native:10');
});

it('passes an explicitly empty native publication to Fabric without inventing memory', async () => {
  const input = preparationInput();
  const hosts = preparationHosts(input, { extractionAnswer: { items: [] } });
  const result = await prepareMemoryPersona(
    input,
    hosts.researchProvider,
    hosts.fabricProvider,
    hosts.openResearch,
    hosts.openFabric,
  );
  expect(result.publication.itemIds).toEqual([]);
  expect(hosts.fabricReservations[0]!.itemIds).toEqual([]);
  expect(result.profile.sourceStatus).toBe('synthetic');
  expect(result.profile.styleExemplars).toEqual([]);
  expect(result.profile.rules).toEqual([]);
  expect(result.profile.memory?.skills).toEqual([]);
  expect(result.profile.archetype).toContain('Недостаточно сведений');
  expect(result.freeze.manifest.sourceRefs).toEqual([]);
});

it('rejects mismatched operation, job, corpus, persona or period scope before any native or model work', async () => {
  const cases: readonly (readonly [string, unknown])[] = [
    ['operationId', 'operation:foreign'],
    ['jobId', 'job:foreign'],
    ['snapshotId', 'corpus:foreign'],
    ['personaId', 'persona:foreign'],
    ['period', { from: 100, to: null, includeUndated: true }],
  ];
  for (const [field, value] of cases) {
    const input = preparationInput();
    const hosts = preparationHosts(input);
    const fabric = { ...input.fabric, [field]: value };
    await expect(
      prepareMemoryPersona(
        preparationInput({ fabric }),
        hosts.researchProvider,
        hosts.fabricProvider,
        hosts.openResearch,
        hosts.openFabric,
      ),
    ).rejects.toThrow('не соответствуют одной области');
    expect(hosts.events).toEqual([]);
  }
  const input = preparationInput();
  const hosts = preparationHosts(input);
  const snapshot = { ...input.snapshot, id: 'corpus:other' };
  await expect(
    prepareMemoryPersona(
      preparationInput({ snapshot }),
      hosts.researchProvider,
      hosts.fabricProvider,
      hosts.openResearch,
      hosts.openFabric,
    ),
  ).rejects.toThrow('Снимок подготовки не соответствует исследованию.');
  expect(hosts.events).toEqual([]);
});

it('rejects manifest model bindings that do not match the requested preparation models before any work', async () => {
  const input = preparationInput();
  const settings = (model: string, effort: 'low' | 'medium' | 'high') =>
    input.manifest.modelSettings.map((entry) =>
      entry.component === 'person-fabric'
        ? { ...entry, requestedModel: model, requestedEffort: effort }
        : entry,
    );
  const candidates: readonly (readonly [PrepareMemoryPersonaInput, string | null])[] = [
    [
      preparationInput({
        manifest: {
          ...input.manifest,
          modelSettings: input.manifest.modelSettings.map((entry) =>
            entry.component === 'stalker' ? { ...entry, requestedModel: 'foreign-model' } : entry,
          ),
        },
      }),
      'не соответствует манифесту',
    ],
    [
      preparationInput({
        manifest: { ...input.manifest, modelSettings: settings('fabric-model', 'low') },
      }),
      'не соответствует манифесту',
    ],
    [
      preparationInput({
        manifest: {
          ...input.manifest,
          modelSettings: settings('fabric-model', 'high').map((entry) =>
            entry.component === 'person-fabric' ? { ...entry, provider: 'kimi' as const } : entry,
          ),
        },
      }),
      'не соответствует манифесту',
    ],
    // Missing and duplicated components are structural manifest decode errors.
    [
      preparationInput({
        manifest: {
          ...input.manifest,
          modelSettings: input.manifest.modelSettings.filter(
            (entry) => entry.component !== 'stalker',
          ),
        },
      }),
      null,
    ],
    [
      preparationInput({
        manifest: {
          ...input.manifest,
          modelSettings: [
            ...input.manifest.modelSettings,
            {
              component: 'stalker' as const,
              provider: 'codex' as const,
              requestedModel: 'research-model',
              requestedEffort: 'medium' as const,
            },
          ],
        },
      }),
      null,
    ],
  ];
  for (const [candidate, message] of candidates) {
    const hosts = preparationHosts(candidate);
    const outcome = prepareMemoryPersona(
      candidate,
      hosts.researchProvider,
      hosts.fabricProvider,
      hosts.openResearch,
      hosts.openFabric,
    );
    if (message) await expect(outcome).rejects.toThrow(message);
    else await expect(outcome).rejects.toThrow();
    expect(hosts.events).toEqual([]);
  }
});

it('rejects invalid fabric-only scope such as palette, skills budget or model budget before research starts', async () => {
  const base = preparationInput();
  const invalid: readonly (readonly [PrepareMemoryPersonaInput, string])[] = [
    [
      preparationInput({ palette: 'orange' as PrepareMemoryPersonaInput['palette'] }),
      'Неверная палитра подготовки памяти.',
    ],
    [preparationInput({ maxSkills: 11 }), 'Неверное число навыков подготовки памяти.'],
    [preparationInput({ maxSkills: 0 }), 'Неверное число навыков подготовки памяти.'],
    [
      preparationInput({
        fabric: {
          ...base.fabric,
          modelBudget: { maxCalls: 1, maxInputBytes: 1000, maxDurationMs: 1000 },
        },
      }),
      'Неверный бюджет модели Fabric.',
    ],
  ];
  for (const [candidate, message] of invalid) {
    const hosts = preparationHosts(candidate);
    await expect(
      prepareMemoryPersona(
        candidate,
        hosts.researchProvider,
        hosts.fabricProvider,
        hosts.openResearch,
        hosts.openFabric,
      ),
    ).rejects.toThrow(message);
    expect(hosts.events).toEqual([]);
  }
});

it('rejects a research provider that does not match the requested research model before any work', async () => {
  const input = preparationInput();
  const hosts = preparationHosts(input);
  const kimiProvider = {
    id: 'kimi' as const,
    run: hosts.researchProvider.run.bind(hosts.researchProvider),
  };
  await expect(
    prepareMemoryPersona(
      input,
      kimiProvider,
      hosts.fabricProvider,
      hosts.openResearch,
      hosts.openFabric,
    ),
  ).rejects.toThrow('CLI не соответствует исследованию.');
  expect(hosts.events).toEqual([]);
});

it('rejects the wrong Fabric provider before spending the research budget', async () => {
  const input = preparationInput();
  const hosts = preparationHosts(input);
  await expect(
    prepareMemoryPersona(
      input,
      hosts.researchProvider,
      { ...hosts.fabricProvider, id: 'kimi' },
      hosts.openResearch,
      hosts.openFabric,
    ),
  ).rejects.toThrow('CLI не соответствует Fabric.');
  expect(hosts.events).toEqual([]);
});

it('projects known inputs before cloning, ignoring unrelated function-valued future task fields', async () => {
  const input = preparationInput();
  const hosts = preparationHosts(input);
  const extended = {
    ...input,
    futureTask: () => 'not preparation data',
    person: { ...input.person, futureTask: () => 'not persona data' },
    limits: { ...input.limits, futureTask: () => 'not a limit' },
  };
  const result = await prepareMemoryPersona(
    extended,
    hosts.researchProvider,
    hosts.fabricProvider,
    hosts.openResearch,
    hosts.openFabric,
  );
  expect(result.profile.label).toBe(input.person.label);
  expect(hosts.researchModelRuns[0]!.task).not.toContain('futureTask');
  expect(hosts.fabricModelRuns[0]!.task).not.toContain('futureTask');
});

it('aborts before the first stage and between stages without opening Fabric', async () => {
  const before = new AbortController();
  before.abort(new Error('stop before start'));
  const input = preparationInput();
  const hosts = preparationHosts(input);
  await expect(
    prepareMemoryPersona(
      input,
      hosts.researchProvider,
      hosts.fabricProvider,
      hosts.openResearch,
      hosts.openFabric,
      before.signal,
    ),
  ).rejects.toThrow('stop before start');
  expect(hosts.events).toEqual([]);

  const between = new AbortController();
  const inputBetween = preparationInput();
  const hostsBetween = preparationHosts(inputBetween, { abortOnResearchClose: between });
  await expect(
    prepareMemoryPersona(
      inputBetween,
      hostsBetween.researchProvider,
      hostsBetween.fabricProvider,
      hostsBetween.openResearch,
      hostsBetween.openFabric,
      between.signal,
    ),
  ).rejects.toThrow('stop between stages');
  expect(hostsBetween.events).toEqual([
    'research:open',
    'research:read:episode:1',
    'research:read:episode:2',
    'research:reserve',
    'research:model',
    'research:publish',
    'research:close',
  ]);
});

it('cancels during research, closes the research session and never opens Fabric', async () => {
  const input = preparationInput();
  const hosts = preparationHosts(input);
  const controller = new AbortController();
  const cancellingPort = {
    ...hosts.researchPort,
    readEpisode: async (episodeId: string, signal?: AbortSignal) => {
      controller.abort(new Error('stop research'));
      return hosts.researchPort.readEpisode(episodeId, signal);
    },
  };
  const openResearch: OpenResearchMemory = async (request) => {
    await hosts.openResearch(request);
    return cancellingPort;
  };
  await expect(
    prepareMemoryPersona(
      input,
      hosts.researchProvider,
      hosts.fabricProvider,
      openResearch,
      hosts.openFabric,
      controller.signal,
    ),
  ).rejects.toThrow('stop research');
  expect(hosts.events).toEqual(['research:open', 'research:read:episode:1', 'research:close']);
});

it('propagates research failure and never opens Fabric after it', async () => {
  const input = preparationInput();
  const hosts = preparationHosts(input);
  const revokedPort = {
    ...hosts.researchPort,
    readEpisode: () => Promise.reject(new Error('source access revoked')),
  };
  const openResearch: OpenResearchMemory = async (request) => {
    await hosts.openResearch(request);
    return revokedPort;
  };
  await expect(
    prepareMemoryPersona(
      input,
      hosts.researchProvider,
      hosts.fabricProvider,
      openResearch,
      hosts.openFabric,
    ),
  ).rejects.toThrow('source access revoked');
  expect(hosts.events).toEqual(['research:open', 'research:close']);
});

it('propagates a Fabric freeze failure without deleting the durable research publication', async () => {
  const input = preparationInput();
  const hosts = preparationHosts(input);
  const frozenPort = {
    ...hosts.fabricPort,
    freezeManifest: () => Promise.reject(new Error('grant revoked')),
  };
  const openFabric: OpenFabricMemory = async (request) => {
    await hosts.openFabric(request);
    return frozenPort;
  };
  await expect(
    prepareMemoryPersona(
      input,
      hosts.researchProvider,
      hosts.fabricProvider,
      hosts.openResearch,
      openFabric,
    ),
  ).rejects.toThrow('grant revoked');
  // The publication already happened; nothing deletes or regenerates it.
  expect(hosts.researchPublications).toHaveLength(1);
  expect(hosts.researchReceipts).toHaveLength(1);
  expect(hosts.events).toContain('research:publish');
  expect(hosts.events).toContain('fabric:close');
  expect(hosts.events).not.toContain('fabric:freeze');
});

it('snapshots mutable options before the first await and never forwards extra caller properties', async () => {
  const input = preparationInput();
  const raced = {
    ...input,
    task: 'FUTURE_TASK_CANARY',
    person: Object.assign(input.person, { task: 'NESTED_TASK_CANARY' }),
  } as PrepareMemoryPersonaInput & { task: string };
  const hosts = preparationHosts(raced);
  const promise = prepareMemoryPersona(
    raced,
    hosts.researchProvider,
    hosts.fabricProvider,
    hosts.openResearch,
    hosts.openFabric,
  );
  Object.assign(raced.person, { label: 'MUTATED' });
  Object.assign(raced.research.model, { model: 'mutated-model', effort: 'low' });
  Object.assign(raced.manifest.modelSettings[0]!, { requestedModel: 'mutated-model' });
  const result = await promise;
  expect(hosts.openResearchRequests[0]!.model.model).toBe('research-model');
  expect(hosts.openFabricRequests[0]!.model.model).toBe('fabric-model');
  expect(
    hosts.fabricFreezes[0]!.modelSettings.find((entry) => entry.component === 'stalker'),
  ).toMatchObject({ requestedModel: 'research-model' });
  expect(result.profile.label).toBe('Анна');
  for (const run of [...hosts.researchModelRuns, ...hosts.fabricModelRuns]) {
    expect(run.task).not.toContain('CANARY');
    expect(run.systemPrompt).not.toContain('CANARY');
  }
  expect(JSON.stringify(result.freeze)).not.toContain('CANARY');
  expect(JSON.stringify(result.publication)).not.toContain('CANARY');
});
