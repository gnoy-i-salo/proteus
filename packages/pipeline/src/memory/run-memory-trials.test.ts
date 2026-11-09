import { beforeEach, expect, it, vi } from 'vitest';
import { createModelSettings } from '@proteus-int/intelligence';
import type { IntelligenceProvider } from '@proteus-int/intelligence';
import { solveMemory } from '@proteus-int/person';
import type * as Person from '@proteus-int/person';
import type { MemoryPersonResult, OpenPersonaMemory } from '@proteus-int/person';
import { decodeV4Result } from '../history/result-v4';
import { createStageRunner } from '../execution/run-stage';
import { runMemoryTrials } from './run-memory-trials';
import type { MemoryTrialsInput } from './run-memory-trials';
import fixture from '../testing/legacy-results/v4-current-with-execution.json';

vi.mock('@proteus-int/person', async (original) => ({
  ...(await original<typeof Person>()),
  solveMemory: vi.fn(),
}));
const solve = vi.mocked(solveMemory);
const open = vi.fn<OpenPersonaMemory>();
const trace = { provider: 'codex' as const, reasoningMs: 0, terminalMs: 0, summary: '' };
function setup() {
  const prior = decodeV4Result(fixture);
  const roles = ['a', 'b', 'neutral'] as const;
  const modelSettings = Object.fromEntries(
    Object.entries(prior.protocol.modelSettings).map(
      ([component, model]) => [component, { ...model, requestedModel: 'pinned-model' }] as const,
    ),
  );
  const participants = [...prior.people, prior.control].map((person, index) => {
    const id = index === 2 ? 'saved-control' : 'shared-identity';
    const styleProfile = {
      version: 2 as const,
      reasoningPatterns: [],
      voiceRules: [],
      formatRules: [],
      contextModes: (['dialogue', 'comment', 'formal'] as const).map((mode) => ({
        mode,
        status: 'insufficient' as const,
        limitations: [],
      })),
      styleExemplarIds: [],
      limitations: [],
    };
    const profile = {
      ...person.baseProfile,
      id,
      skills: [],
      taskSkills: [],
      rules: [],
      styleProfile,
      styleExemplars: [],
      sourceContext: true,
      memory: {
        schemaVersion: 1 as const,
        manifestId: `manifest-${roles[index]}`,
        memorySnapshotId: 'memory',
        skills: [],
      },
    };
    const manifest = {
      schemaVersion: 2 as const,
      id: profile.memory.manifestId,
      personaId: id,
      profileVersion: 'person-memory-profile:2',
      promptVersion: 'person-fabric-memory:2',
      skillIds: [],
      exemplarIds: [],
      corpusSnapshotId: 'corpus',
      memorySnapshotId: 'memory',
      sourceRefs: [],
      splitVersion: 'split:1',
      preparationVersion: 'nativePreparation:1:' + 'a'.repeat(64),
      retrievalVersion: 'nativeMemoryRetrieval:1',
      embeddingFingerprint: null,
      mode: 'agenticRecall' as const,
      recallBudget: {
        maxCalls: 10,
        maxCandidates: 100,
        maxEpisodes: 10,
        maxSourceBytes: 100000,
        maxContextTokens: 10000,
        maxDurationMs: 60000,
      },
      modelCallBudget: { maxCalls: 8, maxInputBytes: 1000000, maxDurationMs: 60000 },
      modelSettings: (['stalker', 'person-fabric', 'person', 'brain-analyzator'] as const).map(
        (component) => ({ component, ...modelSettings[component]! }),
      ),
      createdAt: '2026-11-09T00:00:00.000Z',
      profileSnapshot: {
        schemaVersion: 1 as const,
        label: profile.label,
        palette: profile.palette,
        archetype: profile.archetype,
        rules: [],
        sourceStatus: profile.sourceStatus,
        styleProfile,
        styleExemplars: [],
      },
    };
    return {
      participantId: roles[index]!,
      report: null,
      base: profile,
      profile,
      manifest,
      request: {
        schemaVersion: 1 as const,
        jobId: 'job',
        personaId: id,
        manifestId: profile.memory.manifestId,
        period: { from: null, to: null, includeUndated: true },
      },
    };
  });
  const input: MemoryTrialsInput = {
    task: prior.input.task,
    experimentId: 'series-1',
    maxRetrievalSteps: 3,
    participants,
    protocol: {
      ...prior.protocol,
      modelSettings,
      memory: {
        schemaVersion: 1,
        mode: 'agenticRecall',
        participants: participants.map((person) => ({
          participantId: person.participantId,
          manifestId: person.profile.memory.manifestId,
        })),
        neutralKnowledgeCondition: 'unmatched',
        limitations: ['Knowledge access has not been matched'],
      },
    },
  };
  const run = vi.fn<IntelligenceProvider['run']>(() =>
    Promise.reject(new Error('judge unavailable')),
  );
  const provider: IntelligenceProvider = { id: 'codex', run };
  const execution = createStageRunner(
    {
      settings: createModelSettings(),
      providers: { codex: provider, kimi: { ...provider, id: 'kimi' } },
      sourceContext: true,
      total: 5,
    },
    {},
  );
  return { input, execution, run };
}
beforeEach(() => {
  vi.resetAllMocks();
  solve.mockImplementation((input) => {
    const answer = `Answer for ${input.simulation.trialId}`;
    const value: MemoryPersonResult = {
      answer,
      invocationId: `call-${input.simulation.trialId}`,
      trace,
      traces: [trace],
      retrievals: [],
      contextLeaseIds: ['lease'],
      simulation: {
        schemaVersion: 1,
        id: 'simulation:1',
        ...input.simulation,
        manifestId: input.request.manifestId,
        role: 'simulation',
        text: answer,
        createdAt: '2026-11-09T00:00:00.000Z',
      },
      context: {
        schemaVersion: 2,
        id: 'lease',
        singleUse: true,
        personaId: input.profile.id,
        inputSha256: 'a'.repeat(64),
        inputBytes: 10,
        systemPrompt: 'private native prompt',
        task: 'private authorized source body',
        corpusSnapshotId: 'corpus',
        manifestId: input.request.manifestId,
        purpose: 'personaReply',
        formatAttempt: 1,
        policyVersion: 'policy',
        receiptIds: [],
        items: [],
        sources: [],
        contextSha256: 'b'.repeat(64),
        expiresAt: '2026-11-09T00:10:00.000Z',
        usage: { modelCalls: 1, inputBytes: 10 },
      },
    };
    return Promise.resolve(value);
  });
});
it('uses native memory solving for every role and retains observations apart from judged answers', async () => {
  const { input, execution, run } = setup();
  const result = await runMemoryTrials(input, execution, open);
  expect(solve).toHaveBeenCalledTimes(3);
  expect(result.status).toBe('incomplete');
  expect(
    result.trials.map((trial) => [trial.personId, trial.attempt?.answer, trial.status]),
  ).toEqual([
    ['a', 'Answer for 1-a', 'answered'],
    ['b', 'Answer for 1-b', 'answered'],
    ['neutral', 'Answer for 1-neutral', 'answered'],
  ]);
  expect(result.memoryRuns.map((run) => [run.participantId, run.trialId])).toEqual([
    ['a', '1-a'],
    ['b', '1-b'],
    ['neutral', '1-neutral'],
  ]);
  expect(result.memoryRuns[0]!.memory.context.task).toBe('private authorized source body');
  expect(JSON.stringify(result.trials)).not.toContain('private authorized source body');
  expect(run).toHaveBeenCalledTimes(1);
  const requests = solve.mock.calls.map(([selected]) => selected.request);
  expect(new Set(requests.map((request) => request.operationId)).size).toBe(3);
  expect(requests.map((request) => request.personaId)).toEqual([
    'shared-identity',
    'shared-identity',
    'saved-control',
  ]);
  expect(solve.mock.calls.every(([, , usedOpen]) => usedOpen === open)).toBe(true);
});
it('rejects mismatched protocol and native manifest scopes before any model or session', async () => {
  const { input, execution } = setup();
  const changed = structuredClone(input);
  Object.assign(changed.participants[1]!.request, { manifestId: 'foreign' });
  await expect(runMemoryTrials(changed, execution, open)).rejects.toThrow();
  expect(solve).not.toHaveBeenCalled();
  expect(open).not.toHaveBeenCalled();
});

it('stops the remaining roles after cancellation while retaining an already returned native observation', async () => {
  const { input, execution, run } = setup();
  const controller = new AbortController();
  const original = solve.getMockImplementation()!;
  solve.mockImplementationOnce(async (...args) => {
    const result = await original(...args);
    controller.abort();
    return result;
  });
  const result = await runMemoryTrials(input, execution, open, controller.signal);
  expect(result.status).toBe('cancelled');
  expect(solve).toHaveBeenCalledTimes(1);
  expect(result.memoryRuns).toHaveLength(1);
  expect(result.trials.map((trial) => trial.status)).toEqual(['failed', 'skipped', 'skipped']);
  expect(run).not.toHaveBeenCalled();
});

it('keeps the original first failure and does not run other roles after native denial', async () => {
  const { input, execution, run } = setup();
  solve.mockRejectedValueOnce(new Error('Access revoked'));
  const result = await runMemoryTrials(input, execution, open);
  expect(result.status).toBe('incomplete');
  expect(result.trials.map((trial) => trial.status)).toEqual(['failed', 'skipped', 'skipped']);
  expect(result.trials[0]!.error).toBe('Access revoked');
  expect(result.memoryRuns).toEqual([]);
  expect(solve).toHaveBeenCalledTimes(1);
  expect(run).not.toHaveBeenCalled();
});

it('freezes all participants before calls and keeps nine rotated trials and six blind judgments', async () => {
  const { input, execution, run } = setup();
  const protocol = {
    ...input.protocol,
    repetitions: 3 as const,
    participantOrder: [
      ['a', 'b', 'neutral'],
      ['b', 'neutral', 'a'],
      ['neutral', 'a', 'b'],
    ],
  };
  const chosen = { ...input, protocol };
  const original = solve.getMockImplementation()!;
  solve.mockImplementationOnce((...args) => {
    Object.assign(input.participants[1]!.profile, { id: 'late-mutation' });
    return original(...args);
  });
  run.mockImplementation((request) => {
    const payload = JSON.parse(request.task) as {
      answers: { label: string; answer: string }[];
      rubric: { criteria: { id: string }[] };
    };
    return Promise.resolve({
      trace,
      answer: JSON.stringify({
        evaluations: payload.answers.map((answer) => ({
          label: answer.label,
          criteria: payload.rubric.criteria.map((criterion) => ({
            criterionId: criterion.id,
            status: 'partial',
            reason: 'Synthetic observation',
            quote: answer.answer,
          })),
        })),
      }),
    });
  });
  const result = await runMemoryTrials(chosen, execution, open);
  expect(result.status).toBe('complete');
  expect(result.trials).toHaveLength(9);
  expect(result.trials.every((trial) => trial.status === 'evaluated')).toBe(true);
  expect(result.trials.map((trial) => trial.personId)).toEqual(protocol.participantOrder.flat());
  expect(solve).toHaveBeenCalledTimes(9);
  expect(run).toHaveBeenCalledTimes(6);
  expect(result.memoryRuns).toHaveLength(9);
  expect(new Set(solve.mock.calls.map(([selected]) => selected.request.operationId)).size).toBe(9);
  expect(
    solve.mock.calls.filter(([selected]) => selected.request.personaId === 'shared-identity'),
  ).toHaveLength(6);
  expect(solve.mock.calls.every(([selected]) => selected.profile.id !== 'late-mutation')).toBe(
    true,
  );
  expect(solve.mock.calls.every(([selected]) => Object.isFrozen(selected.expectedManifest))).toBe(
    true,
  );
});

it.each([
  'memoryVersion',
  'skillBody',
  'baseline',
  'model',
  'neutral',
  'mode',
  'taskAdaptation',
  'period',
])('preflights every participant before any call: %s', async (field) => {
  const { input, execution } = setup();
  const value = structuredClone(input);
  const person = value.participants[2]!;
  if (field === 'memoryVersion') Object.assign(person.profile.memory!, { schemaVersion: 99 });
  if (field === 'skillBody') {
    Object.assign(person.profile.memory!, { skills: [{ id: 'forged' }] });
    Object.assign(person.manifest, { skillIds: ['forged'] });
  }
  if (field === 'baseline') Object.assign(person, { base: { ...person.base, archetype: 'other' } });
  if (field === 'model')
    Object.assign(person.manifest.modelSettings[0]!, { requestedModel: 'other' });
  if (field === 'neutral') Object.assign(person.profile, { sourceStatus: 'synthetic' });
  if (field === 'mode') Object.assign(person.manifest, { mode: 'fixedContext' });
  if (field === 'taskAdaptation')
    Object.assign(person, {
      taskAdaptation: {
        schemaVersion: 1,
        manifestId: person.manifest.id,
        task: 'other',
        strategies: [],
      },
    });
  if (field === 'period') Object.assign(person.request.period, { from: 'invalid' });
  await expect(runMemoryTrials(value, execution, open)).rejects.toThrow();
  expect(solve).not.toHaveBeenCalled();
  expect(open).not.toHaveBeenCalled();
});
