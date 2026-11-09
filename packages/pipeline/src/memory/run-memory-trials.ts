import type {
  ExperimentProtocol,
  OpenPersonaSessionRequest,
  PersonaManifest,
} from '@proteus-int/contracts';
import {
  decodeOpenPersonaSessionRequest,
  decodePersonaManifest,
  decodePersonaProfileSnapshot,
  decodePersonaSimulationScope,
  decodePersonaTaskAdaptation,
  decodeSkillDefinition,
} from '@proteus-int/contracts';
import { solveMemory } from '@proteus-int/person';
import type { MemoryPersonResult, OpenPersonaMemory } from '@proteus-int/person';
import type { PreparedPerson } from '../experiment/prepare-people';
import type { ExperimentResult, TrialResult } from '../experiment/result';
import type { createStageRunner } from '../execution/run-stage';
import { freeze } from '../experiment/input-snapshot';
import { buildTrialOrder } from '../experiment/protocol';
import { runTrials } from '../experiment/run-trials';

export interface MemoryTrialParticipant extends PreparedPerson {
  readonly manifest: Extract<PersonaManifest, { readonly schemaVersion: 2 }>;
  readonly request: Omit<OpenPersonaSessionRequest, 'operationId'>;
}
export interface MemoryTrialsInput {
  readonly task: string;
  readonly protocol: ExperimentProtocol;
  readonly participants: readonly MemoryTrialParticipant[];
  readonly experimentId: string;
  readonly maxRetrievalSteps: number;
}
export interface MemoryTrialRun {
  readonly trialId: string;
  readonly participantId: 'a' | 'b' | 'neutral';
  readonly memory: MemoryPersonResult;
}
export interface MemoryTrialsResult {
  readonly trials: readonly TrialResult[];
  readonly status: ExperimentResult['status'];
  /** Live authorized observations; do not serialize as an unrestricted public export. */
  readonly memoryRuns: readonly MemoryTrialRun[];
}
function validateModels(manifest: PersonaManifest, protocol: ExperimentProtocol) {
  for (const model of manifest.modelSettings) {
    const setting = protocol.modelSettings[model.component];
    if (
      !setting ||
      model.provider !== setting.provider ||
      model.requestedModel !== setting.requestedModel ||
      model.requestedEffort !== setting.requestedEffort
    )
      throw new Error('Настройки протокола не соответствуют манифесту персоны.');
  }
}
function validateParticipant(person: MemoryTrialParticipant, protocol: ExperimentProtocol) {
  const manifest = decodePersonaManifest(person.manifest);
  const request = decodeOpenPersonaSessionRequest({
    ...person.request,
    operationId: crypto.randomUUID(),
  });
  const profile = person.profile;
  const binding = protocol.memory!.participants.find(
    (entry) => entry.participantId === person.participantId,
  );
  if (
    manifest.schemaVersion !== 2 ||
    !binding ||
    manifest.id !== binding.manifestId ||
    manifest.id !== request.manifestId ||
    manifest.personaId !== request.personaId ||
    profile.id !== manifest.personaId ||
    profile.memory?.manifestId !== manifest.id ||
    profile.memory.schemaVersion !== 1 ||
    profile.memory.memorySnapshotId !== manifest.memorySnapshotId ||
    manifest.mode !== protocol.memory!.mode
  )
    throw new Error('Профиль и запрос памяти не соответствуют протоколу.');
  const snapshot = decodePersonaProfileSnapshot({
    schemaVersion: 1,
    label: profile.label,
    palette: profile.palette,
    archetype: profile.archetype,
    rules: profile.rules,
    sourceStatus: profile.sourceStatus,
    styleProfile: profile.styleProfile,
    styleExemplars: profile.styleExemplars,
  });
  if (
    JSON.stringify(snapshot) !== JSON.stringify(manifest.profileSnapshot) ||
    JSON.stringify(person.base) !== JSON.stringify(profile) ||
    profile.sourceContext !== true ||
    profile.skills.length ||
    profile.taskSkills.length ||
    JSON.stringify(profile.memory.skills.map(decodeSkillDefinition).map((skill) => skill.id)) !==
      JSON.stringify(manifest.skillIds) ||
    (person.participantId === 'neutral' && profile.sourceStatus !== 'neutral')
  )
    throw new Error('Серия требует неизменённые базовые профили памяти и нейтральный контроль.');
  validateModels(manifest, protocol);
  if (person.taskAdaptation)
    decodePersonaTaskAdaptation(person.taskAdaptation, {
      manifestId: manifest.id,
      task: protocol.task,
      profile: snapshot,
    });
  return { ...person, manifest, request };
}
function select(input: MemoryTrialsInput) {
  const roles = ['a', 'b', 'neutral'];
  const memory = input.protocol.memory;
  if (
    input.task !== input.protocol.task ||
    !memory ||
    memory.schemaVersion !== 1 ||
    input.participants.length !== 3 ||
    input.participants.some((person, index) => person.participantId !== roles[index]) ||
    memory.participants.length !== 3 ||
    new Set(memory.participants.map((entry) => entry.participantId)).size !== 3 ||
    JSON.stringify(input.protocol.participantOrder) !==
      JSON.stringify(buildTrialOrder(input.protocol.repetitions)) ||
    !Number.isSafeInteger(input.maxRetrievalSteps) ||
    input.maxRetrievalSteps < 0 ||
    input.maxRetrievalSteps > 64
  )
    throw new Error('Неверные закреплённые условия серии с памятью.');
  decodePersonaSimulationScope({ experimentId: input.experimentId, trialId: 'preflight' });
  return freeze(
    structuredClone({
      task: input.task,
      protocol: input.protocol,
      experimentId: input.experimentId,
      maxRetrievalSteps: input.maxRetrievalSteps,
      participants: input.participants.map((person) => validateParticipant(person, input.protocol)),
    }),
  );
}
export async function runMemoryTrials(
  input: MemoryTrialsInput,
  execution: ReturnType<typeof createStageRunner>,
  open: OpenPersonaMemory,
  signal?: AbortSignal,
): Promise<MemoryTrialsResult> {
  signal?.throwIfAborted();
  const selected = select(input);
  const memoryRuns: MemoryTrialRun[] = [];
  const outcome = await runTrials(
    selected.task,
    selected.protocol,
    selected.participants,
    execution,
    signal ? { signal } : {},
    async (participant, trial, provider) => {
      signal?.throwIfAborted();
      const chosen = selected.participants.find(
        (person) => person.participantId === participant.participantId,
      )!;
      const memory = await solveMemory(
        {
          profile: chosen.profile,
          task: selected.task,
          request: { ...chosen.request, operationId: crypto.randomUUID() },
          expectedManifest: chosen.manifest,
          maxRetrievalSteps: selected.maxRetrievalSteps,
          simulation: { experimentId: selected.experimentId, trialId: trial.id },
          ...(chosen.taskAdaptation ? { adaptation: chosen.taskAdaptation } : {}),
        },
        provider,
        open,
        signal,
      );
      memoryRuns.push(
        freeze(structuredClone({ participantId: chosen.participantId, trialId: trial.id, memory })),
      );
      signal?.throwIfAborted();
      return {
        answer: memory.answer,
        trace: memory.trace,
        ...(memory.invocationId ? { invocationId: memory.invocationId } : {}),
        ...(memory.reply ? { reply: memory.reply } : {}),
      };
    },
  );
  return { ...outcome, memoryRuns };
}
