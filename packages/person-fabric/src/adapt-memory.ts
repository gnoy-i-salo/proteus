import type {
  ManifestFreezeReceipt,
  OpenFabricSessionRequest,
  PersonaTaskAdaptation,
  IntelligenceTrace,
} from '@proteus-int/contracts';
import type { IntelligenceProvider } from '@proteus-int/intelligence';
import type { OpenFabricMemory } from './fabric-session';
import {
  decodeOpenFabricSessionRequest,
  decodePersonaManifest,
  decodeSkillDefinition,
  decodeFabricContextRequest,
  decodePersonaTaskAdaptation,
} from '@proteus-int/contracts';
import { withFabricSession } from './fabric-session';
import { profileFromPublication } from './memory-fabric';
import { adaptPerson } from './adapt-person';

export interface MemoryAdaptationInput {
  readonly publication: ManifestFreezeReceipt;
  readonly request: OpenFabricSessionRequest;
  readonly task: string;
}
export interface MemoryAdaptationResult {
  readonly adaptation: PersonaTaskAdaptation;
  readonly traces: readonly IntelligenceTrace[];
}
export async function adaptMemory(
  input: MemoryAdaptationInput,
  provider: IntelligenceProvider,
  open: OpenFabricMemory,
  signal?: AbortSignal,
): Promise<MemoryAdaptationResult> {
  signal?.throwIfAborted();
  const request = decodeOpenFabricSessionRequest(input.request);
  const manifest = decodePersonaManifest(input.publication.manifest);
  const skills = input.publication.skills.map(decodeSkillDefinition);
  const model = manifest.modelSettings.find((entry) => entry.component === 'person-fabric')!;
  if (
    input.publication.schemaVersion !== 1 ||
    manifest.schemaVersion !== 2 ||
    manifest.profileVersion !== 'person-memory-profile:2' ||
    manifest.promptVersion !== 'person-fabric-memory:2' ||
    request.personaId !== manifest.personaId ||
    request.snapshotId !== manifest.corpusSnapshotId ||
    request.modelBudget.maxCalls < 2 ||
    request.model.provider !== model.provider ||
    request.model.model !== model.requestedModel ||
    request.model.effort !== model.requestedEffort ||
    JSON.stringify(skills.map((skill) => skill.id)) !== JSON.stringify(manifest.skillIds)
  )
    throw new Error('Адаптация не соответствует закреплённому профилю или модели.');
  const profile = profileFromPublication({ schemaVersion: 1, manifest, skills });
  const selection = decodeFabricContextRequest({
    schemaVersion: 1,
    memorySnapshotId: manifest.memorySnapshotId,
    itemIds: [
      ...new Set([
        ...profile.rules.flatMap((rule) => rule.evidenceIds),
        ...skills.flatMap((skill) => skill.memoryItemIds),
        ...manifest.exemplarIds,
      ]),
    ],
    task: input.task,
    systemPrompt: '',
    formatAttempt: 1,
  });
  const task = selection.task;
  if (new TextEncoder().encode(task).length > 100000)
    throw new Error('Превышен размер задачи Person.');
  return withFabricSession(
    request,
    selection,
    provider,
    open,
    signal,
    async (model, _memory, sessionSignal) => {
      const adapted = await adaptPerson(profile, task, model.provider, sessionSignal);
      sessionSignal.throwIfAborted();
      const adaptation = decodePersonaTaskAdaptation(
        {
          schemaVersion: 1,
          manifestId: manifest.id,
          task,
          strategies: adapted.profile.taskSkills.map((rule) => ({ ...rule, status: 'hypothesis' })),
        },
        { manifestId: manifest.id, task, profile },
      );
      return { adaptation, traces: model.traces };
    },
  );
}
