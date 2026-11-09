import snapshotFixture from './persona-profile-snapshot.json';
import { decodePersonaProfileSnapshot } from '../persona-profile-snapshot';
import type { ManifestFreezeReceipt, PersonaManifestDraft, SkillDefinition } from '../memory';
import { fabricContextRequest, fabricLease, fabricRequest, fabricSession } from './fabric-fixtures';

export function manifestDraft(): PersonaManifestDraft {
  const lease = fabricLease();
  return {
    schemaVersion: 1,
    profileVersion: 'personFabricMemory:1',
    memorySnapshotId: fabricContextRequest.memorySnapshotId,
    contextLeaseIds: [lease.id],
    exemplarIds: [lease.items[0]!.id],
    promptVersion: 'fabricMemory:1',
    mode: 'agenticRecall',
    recallBudget: { ...fabricRequest.recallBudget },
    modelCallBudget: { ...fabricRequest.modelBudget },
    modelSettings: (['stalker', 'person-fabric', 'person', 'brain-analyzator'] as const).map(
      (component) => ({
        component,
        provider: 'codex',
        requestedModel: 'fixture-model',
        requestedEffort: 'medium',
      }),
    ),
    skills: [
      {
        draftId: 'suggestion:1',
        trigger: '\uFEFF  Если просят проверить текст\r\n',
        inputs: [],
        steps: ['  Сверить запись é 👩🏽‍🔬\r\n'],
        outcomeCheck: 'Сохранён исходный текст',
        support: 'proposed',
        memoryItemIds: [lease.items[0]!.id],
        sourceSpans: [...lease.items[0]!.sourceSpans],
        limitations: ['Предложение, не наблюдаемый навык.'],
      },
    ],
  };
}
export function frozenManifest(): ManifestFreezeReceipt {
  const draft = manifestDraft();
  const skill = draft.skills[0]!;
  const native: SkillDefinition = {
    schemaVersion: 1,
    id: 'memorySkill:1',
    trigger: skill.trigger,
    inputs: skill.inputs,
    steps: skill.steps,
    outcomeCheck: skill.outcomeCheck,
    support: skill.support,
    memoryItemIds: skill.memoryItemIds,
    sourceSpans: skill.sourceSpans,
    limitations: skill.limitations,
  };
  const { span } = fabricLease().sources[0]!;
  return {
    schemaVersion: 1,
    manifest: {
      schemaVersion: 1,
      id: 'personaManifest:1',
      personaId: fabricSession.personaId,
      profileVersion: draft.profileVersion,
      skillIds: [native.id],
      exemplarIds: draft.exemplarIds,
      corpusSnapshotId: fabricSession.corpusSnapshotId,
      memorySnapshotId: draft.memorySnapshotId,
      sourceRefs: [{ documentId: span.documentId, revisionId: span.revisionId }],
      splitVersion: 'episodeSplit:1',
      preparationVersion: 'nativePreparation:1:' + 'a'.repeat(64),
      retrievalVersion: 'nativeMemoryRetrieval:1',
      promptVersion: draft.promptVersion,
      embeddingFingerprint: null,
      mode: draft.mode,
      recallBudget: draft.recallBudget,
      modelCallBudget: draft.modelCallBudget,
      modelSettings: draft.modelSettings,
      createdAt: '2026-11-09T03:00:01.004Z',
    },
    skills: [native],
  };
}

/** Synthetic bound V2 bundle; the original V1 fixture remains unchanged. */
export function boundManifest(): ManifestFreezeReceipt {
  const previous = frozenManifest();
  const snapshot = structuredClone(snapshotFixture);
  const id = previous.manifest.exemplarIds[0]!;
  snapshot.styleExemplars[0]!.id = id;
  snapshot.styleProfile.styleExemplarIds = [id];
  snapshot.styleProfile.voiceRules[0]!.exemplarIds = [id];
  return {
    ...previous,
    manifest: {
      ...previous.manifest,
      schemaVersion: 2,
      profileVersion: 'person-memory-profile:2',
      promptVersion: 'person-fabric-memory:2',
      profileSnapshot: decodePersonaProfileSnapshot(snapshot),
    },
  };
}
