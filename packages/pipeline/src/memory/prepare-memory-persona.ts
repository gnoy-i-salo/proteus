import type {
  CorpusSnapshot,
  IntelligenceTrace,
  ManifestFreezeReceipt,
  PaletteId,
  PersonInput,
  PersonProfile,
  PersonaManifestDraft,
  ResearchPublicationReceipt,
  ResearchSample,
} from '@proteus-int/contracts';
import {
  decodeCorpusSnapshot,
  decodeOpenFabricSessionRequest,
  decodeOpenResearchSessionRequest,
  decodePersonaManifestDraft,
} from '@proteus-int/contracts';
import type { OpenFabricSessionRequest, OpenResearchSessionRequest } from '@proteus-int/contracts';
import type { IntelligenceProvider } from '@proteus-int/intelligence';
import { fabricateMemory } from '@proteus-int/person-fabric';
import type { MemoryFabricInput, OpenFabricMemory } from '@proteus-int/person-fabric';
import { researchMemory } from '@proteus-int/stalker';
import type { MemoryResearchInput, OpenResearchMemory } from '@proteus-int/stalker';
import { freeze } from '../experiment/input-snapshot';

export interface PrepareMemoryPersonaLimits {
  readonly maxSeedEpisodes: number;
  readonly maxPlanningSteps: number;
  readonly maxItemsPerBatch: number;
}

export interface PrepareMemoryPersonaInput {
  readonly person: PersonInput;
  readonly palette: PaletteId;
  readonly snapshot: CorpusSnapshot;
  readonly samples?: readonly ResearchSample[];
  readonly limits: PrepareMemoryPersonaLimits;
  readonly research: OpenResearchSessionRequest;
  readonly fabric: OpenFabricSessionRequest;
  readonly maxSkills: number;
  readonly manifest: Pick<
    PersonaManifestDraft,
    'mode' | 'recallBudget' | 'modelCallBudget' | 'modelSettings'
  >;
}

export interface PreparedMemoryPersona {
  readonly publication: ResearchPublicationReceipt;
  readonly researchLimitations: readonly string[];
  readonly researchTraces: readonly IntelligenceTrace[];
  readonly profile: PersonProfile;
  readonly freeze: ManifestFreezeReceipt;
  readonly fabricTraces: readonly IntelligenceTrace[];
}

// Placeholder draft versions mirror the Fabric stage writer exactly: the
// pre-flight decodes the same manifest shape the freeze will persist.
const preparationManifestProfileVersion = 'person-memory-profile:2';
const preparationManifestPromptVersion = 'person-fabric-memory:2';
const preparationPlaceholderId = 'pending:validation';

interface SelectedPreparation {
  readonly person: PersonInput;
  readonly palette: PaletteId;
  readonly snapshot: CorpusSnapshot;
  readonly samples: readonly ResearchSample[] | undefined;
  readonly limits: PrepareMemoryPersonaLimits;
  readonly research: OpenResearchSessionRequest;
  readonly fabric: OpenFabricSessionRequest;
  readonly maxSkills: number;
  readonly manifest: PersonaManifestDraft;
}

// All cross-stage checks run before any native session or model call: the two
// stages must share one operation/job/corpus/persona/period scope, and the
// manifest model bindings must match the requested preparation models.
function validatePreparation(input: PrepareMemoryPersonaInput): SelectedPreparation {
  const research = decodeOpenResearchSessionRequest(input.research);
  const fabric = decodeOpenFabricSessionRequest(input.fabric);
  const snapshot = decodeCorpusSnapshot(input.snapshot);
  if (snapshot.id !== research.snapshotId)
    throw new Error('Снимок подготовки не соответствует исследованию.');
  if (
    research.operationId !== fabric.operationId ||
    research.jobId !== fabric.jobId ||
    research.snapshotId !== fabric.snapshotId ||
    research.personaId !== fabric.personaId ||
    research.period.from !== fabric.period.from ||
    research.period.to !== fabric.period.to ||
    research.period.includeUndated !== fabric.period.includeUndated
  )
    throw new Error('Исследование и Fabric не соответствуют одной области подготовки.');
  if (input.palette !== 'turquoise' && input.palette !== 'violet')
    throw new Error('Неверная палитра подготовки памяти.');
  if (!Number.isSafeInteger(input.maxSkills) || input.maxSkills < 1 || input.maxSkills > 10)
    throw new Error('Неверное число навыков подготовки памяти.');
  if (fabric.modelBudget.maxCalls < 2) throw new Error('Неверный бюджет модели Fabric.');
  const manifest = decodePersonaManifestDraft({
    schemaVersion: 1,
    profileVersion: preparationManifestProfileVersion,
    promptVersion: preparationManifestPromptVersion,
    memorySnapshotId: preparationPlaceholderId,
    contextLeaseIds: [preparationPlaceholderId],
    exemplarIds: [],
    skills: [],
    mode: input.manifest.mode,
    recallBudget: input.manifest.recallBudget,
    modelCallBudget: input.manifest.modelCallBudget,
    modelSettings: input.manifest.modelSettings,
  });
  const researchModel = manifest.modelSettings.find((entry) => entry.component === 'stalker');
  if (
    !researchModel ||
    researchModel.provider !== research.model.provider ||
    researchModel.requestedModel !== research.model.model ||
    researchModel.requestedEffort !== research.model.effort
  )
    throw new Error('Модель исследования не соответствует манифесту подготовки.');
  const fabricModel = manifest.modelSettings.find((entry) => entry.component === 'person-fabric');
  if (
    !fabricModel ||
    fabricModel.provider !== fabric.model.provider ||
    fabricModel.requestedModel !== fabric.model.model ||
    fabricModel.requestedEffort !== fabric.model.effort
  )
    throw new Error('Модель Fabric не соответствует манифесту подготовки.');
  return {
    person: {
      label: input.person.label,
      context: input.person.context,
      publicUrls: [...input.person.publicUrls],
    },
    palette: input.palette,
    snapshot,
    samples: input.samples?.map((sample) => ({
      episodeId: sample.episodeId,
      sourceKey: sample.sourceKey,
      periodKey: sample.periodKey,
    })),
    limits: {
      maxSeedEpisodes: input.limits.maxSeedEpisodes,
      maxPlanningSteps: input.limits.maxPlanningSteps,
      maxItemsPerBatch: input.limits.maxItemsPerBatch,
    },
    research,
    fabric,
    maxSkills: input.maxSkills,
    manifest,
  };
}

// Serial task-free preparation of ONE persona: STALKER research publishes
// memory natively first; Person Fabric then fabricates a profile from that
// native publication. Callers cannot supply memory ids: the published
// snapshot id and the ordered published item ids drive Fabric. Session close,
// cancellation and deadlines stay with the existing stage controllers.
export async function prepareMemoryPersona(
  input: PrepareMemoryPersonaInput,
  researchProvider: IntelligenceProvider,
  fabricProvider: IntelligenceProvider,
  openResearch: OpenResearchMemory,
  openFabric: OpenFabricMemory,
  signal?: AbortSignal,
): Promise<PreparedMemoryPersona> {
  signal?.throwIfAborted();
  const selected = freeze(structuredClone(validatePreparation(input)));
  if (researchProvider.id !== selected.research.model.provider)
    throw new Error('CLI не соответствует исследованию.');
  if (fabricProvider.id !== selected.fabric.model.provider)
    throw new Error('CLI не соответствует Fabric.');
  const researchInput: MemoryResearchInput = {
    person: selected.person,
    request: selected.research,
    snapshot: selected.snapshot,
    ...(selected.samples === undefined ? {} : { samples: selected.samples }),
    limits: selected.limits,
  };
  const researched = await researchMemory(researchInput, researchProvider, openResearch, signal);
  signal?.throwIfAborted();
  const fabricInput: MemoryFabricInput = {
    person: selected.person,
    palette: selected.palette,
    request: selected.fabric,
    memorySnapshotId: researched.publication.snapshot.id,
    itemIds: researched.publication.itemIds.map((published) => published.itemId),
    maxSkills: selected.maxSkills,
    manifest: {
      mode: selected.manifest.mode,
      recallBudget: selected.manifest.recallBudget,
      modelCallBudget: selected.manifest.modelCallBudget,
      modelSettings: selected.manifest.modelSettings,
    },
  };
  const fabricated = await fabricateMemory(fabricInput, fabricProvider, openFabric, signal);
  return {
    publication: researched.publication,
    researchLimitations: researched.limitations,
    researchTraces: researched.traces,
    profile: fabricated.profile,
    freeze: fabricated.publication,
    fabricTraces: fabricated.traces,
  };
}
