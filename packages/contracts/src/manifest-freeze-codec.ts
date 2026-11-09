import { decodePersonaProfileSnapshot } from './persona-profile-snapshot';
import type { ManifestFreezeReceipt, PersonaManifestDraft } from './memory';
import type { RevisionReference } from './corpus';
import type { FabricSessionDescriptor } from './fabric-memory';
import { choice, fields } from './corpus-wire-validation';
import { memoryId, memoryIds, memoryList, memoryTimestamp } from './memory-wire-validation';
import {
  decodeManifestReferences,
  decodeManifestSettings,
  decodePersonaManifest,
  decodeSkillDefinition,
  decodeSkillDraft,
} from './memory-profile-codec';
import { decodeModelCallBudget, decodeRecallBudget } from './research-memory-codec';

export function decodePersonaManifestDraft(value: unknown): PersonaManifestDraft {
  const bound =
    value !== null &&
    typeof value === 'object' &&
    'schemaVersion' in value &&
    value.schemaVersion === 2;
  const input = fields(value, [
    ...(bound ? ['profileSnapshot'] : []),
    'schemaVersion',
    'profileVersion',
    'memorySnapshotId',
    'contextLeaseIds',
    'exemplarIds',
    'promptVersion',
    'mode',
    'recallBudget',
    'modelCallBudget',
    'modelSettings',
    'skills',
  ]);
  if (input.schemaVersion !== 1 && input.schemaVersion !== 2)
    throw new Error('Неизвестная версия запроса манифеста.');
  const version =
    input.schemaVersion === 1
      ? { schemaVersion: 1 as const }
      : {
          schemaVersion: 2 as const,
          profileSnapshot: decodePersonaProfileSnapshot(input.profileSnapshot),
        };
  const exemplarIds = memoryIds(memoryList(input.exemplarIds, 100));
  if (
    version.schemaVersion === 2 &&
    JSON.stringify(exemplarIds) !==
      JSON.stringify(version.profileSnapshot.styleExemplars.map((sample) => sample.id))
  )
    throw new Error('Черновик изменяет состав образцов профиля.');
  const skills = memoryList(input.skills, 64).map(decodeSkillDraft);
  if (new Set(skills.map((skill) => skill.draftId)).size !== skills.length)
    throw new Error('Повторный черновик навыка.');
  return {
    ...version,
    profileVersion: memoryId(input.profileVersion),
    memorySnapshotId: memoryId(input.memorySnapshotId),
    contextLeaseIds: memoryIds(memoryList(input.contextLeaseIds, 1000, 1)),
    exemplarIds,
    promptVersion: memoryId(input.promptVersion),
    mode: choice(input.mode, ['fixedContext', 'agenticRecall']),
    recallBudget: decodeRecallBudget(input.recallBudget),
    modelCallBudget: decodeModelCallBudget(input.modelCallBudget),
    modelSettings: decodeManifestSettings(input.modelSettings),
    skills,
  };
}
/** Checks receipt agreement, not completion proof; native alone owns that authority. */
export function decodeManifestFreeze(
  value: unknown,
  session: FabricSessionDescriptor,
  draft: PersonaManifestDraft,
  sourceRefs: readonly RevisionReference[],
): ManifestFreezeReceipt {
  const expected = decodePersonaManifestDraft(draft);
  const input = fields(value, ['schemaVersion', 'manifest', 'skills']);
  if (input.schemaVersion !== 1 || session.purpose !== 'fabric' || session.mode !== 'lexical')
    throw new Error('Неверная версия или сессия сохранения манифеста.');
  const manifest = decodePersonaManifest(input.manifest);
  const skills = memoryList(input.skills, 64).map(decodeSkillDefinition);
  memoryTimestamp(manifest.createdAt);
  if (
    manifest.schemaVersion !== expected.schemaVersion ||
    (manifest.schemaVersion === 2 &&
      expected.schemaVersion === 2 &&
      JSON.stringify(manifest.profileSnapshot) !== JSON.stringify(expected.profileSnapshot)) ||
    manifest.personaId !== session.personaId ||
    manifest.corpusSnapshotId !== session.corpusSnapshotId ||
    manifest.memorySnapshotId !== expected.memorySnapshotId ||
    manifest.profileVersion !== expected.profileVersion ||
    manifest.promptVersion !== expected.promptVersion ||
    manifest.mode !== expected.mode ||
    manifest.retrievalVersion !== 'nativeMemoryRetrieval:1' ||
    !/^nativePreparation:1:[a-f0-9]{64}$/u.test(manifest.preparationVersion) ||
    Date.parse(manifest.createdAt) >= Date.parse(session.expiresAt) ||
    skills.length !== expected.skills.length
  )
    throw new Error('Манифест не соответствует подготовке Fabric.');
  for (const field of ['exemplarIds', 'recallBudget', 'modelCallBudget', 'modelSettings'] as const)
    if (JSON.stringify(manifest[field]) !== JSON.stringify(expected[field]))
      throw new Error('Манифест изменяет настройки подготовки.');
  if (
    JSON.stringify(manifest.sourceRefs) !== JSON.stringify(decodeManifestReferences(sourceRefs)) ||
    JSON.stringify(manifest.skillIds) !== JSON.stringify(skills.map((skill) => skill.id))
  )
    throw new Error('Манифест изменяет источники или состав навыков.');
  skills.forEach((skill, index) => {
    const body = expected.skills[index]!;
    const materialized = decodeSkillDefinition({
      schemaVersion: 1,
      id: skill.id,
      trigger: body.trigger,
      inputs: body.inputs,
      steps: body.steps,
      outcomeCheck: body.outcomeCheck,
      support: body.support,
      memoryItemIds: body.memoryItemIds,
      sourceSpans: body.sourceSpans,
      limitations: body.limitations,
    });
    if (JSON.stringify(skill) !== JSON.stringify(materialized))
      throw new Error('Сохранение изменяет текст или основания навыка.');
  });
  return { schemaVersion: 1, manifest, skills };
}
