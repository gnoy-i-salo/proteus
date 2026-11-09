import type {
  OpenFabricSessionRequest,
  PaletteId,
  PersonInput,
  PersonaManifestDraft,
  ManifestFreezeReceipt,
  PersonProfile,
  IntelligenceTrace,
  FabricContextRequest,
} from '@proteus-int/contracts';
import {
  decodeOpenFabricSessionRequest,
  decodePersonaManifestDraft,
  decodeFabricContextRequest,
  decodeManifestFreeze,
  decodePersonaProfileSnapshot,
} from '@proteus-int/contracts';
import type { IntelligenceProvider } from '@proteus-int/intelligence';
import { structured } from '@proteus-int/intelligence';
import { withFabricSession } from './fabric-session';
import type { OpenFabricMemory } from './fabric-session';
import { parseMemoryProfile } from './parse-memory-profile';
import { memoryProfileInstructions } from './memory-fabric-prompts';

export interface MemoryFabricInput {
  readonly person: PersonInput;
  readonly palette: PaletteId;
  readonly request: OpenFabricSessionRequest;
  readonly memorySnapshotId: string;
  readonly itemIds: readonly string[];
  readonly maxSkills: number;
  readonly manifest: Pick<
    PersonaManifestDraft,
    'mode' | 'recallBudget' | 'modelCallBudget' | 'modelSettings'
  >;
}
export interface MemoryFabricResult {
  readonly profile: PersonProfile;
  readonly publication: ManifestFreezeReceipt;
  readonly traces: readonly IntelligenceTrace[];
}
function hostText(value: unknown, maximum: number, nonblank = false): string {
  if (typeof value !== 'string') throw new Error('Неверный текст параметров Fabric.');
  const bytes = new TextEncoder().encode(value);
  if (
    bytes.length > maximum ||
    new TextDecoder('utf-8', { fatal: true, ignoreBOM: true }).decode(bytes) !== value ||
    (nonblank && !value.trim())
  )
    throw new Error('Неверный текст параметров Fabric.');
  return value;
}
function validate(input: MemoryFabricInput) {
  const request = decodeOpenFabricSessionRequest(input.request);
  const scope: FabricContextRequest = decodeFabricContextRequest({
    schemaVersion: 1,
    memorySnapshotId: input.memorySnapshotId,
    itemIds: input.itemIds,
    systemPrompt: '',
    task: '',
    formatAttempt: 1,
  });
  if (
    !Number.isSafeInteger(input.maxSkills) ||
    input.maxSkills < 1 ||
    input.maxSkills > 10 ||
    (input.palette !== 'turquoise' && input.palette !== 'violet') ||
    request.modelBudget.maxCalls < 2 ||
    !Array.isArray(input.person.publicUrls) ||
    input.person.publicUrls.length > 32
  )
    throw new Error('Неверная область подготовки Fabric.');
  const person = {
    label: hostText(input.person.label, 512, true),
    context: hostText(input.person.context, 20000),
    publicUrls: input.person.publicUrls.map((url) => hostText(url, 2048, true)),
  };
  hostText(JSON.stringify(person), 30000);
  const draft = decodePersonaManifestDraft({
    schemaVersion: 1,
    profileVersion: 'person-memory-profile:2',
    promptVersion: 'person-fabric-memory:2',
    memorySnapshotId: scope.memorySnapshotId,
    contextLeaseIds: ['pending:validation'],
    exemplarIds: [],
    skills: [],
    mode: input.manifest.mode,
    recallBudget: input.manifest.recallBudget,
    modelCallBudget: input.manifest.modelCallBudget,
    modelSettings: input.manifest.modelSettings,
  });
  const model = draft.modelSettings.find((entry) => entry.component === 'person-fabric')!;
  if (
    model.provider !== request.model.provider ||
    model.requestedModel !== request.model.model ||
    model.requestedEffort !== request.model.effort
  )
    throw new Error('Модель Fabric не соответствует манифесту.');
  return { request, scope, person, draft, palette: input.palette, maxSkills: input.maxSkills };
}
export async function fabricateMemory(
  input: MemoryFabricInput,
  provider: IntelligenceProvider,
  open: OpenFabricMemory,
  signal?: AbortSignal,
): Promise<MemoryFabricResult> {
  signal?.throwIfAborted();
  const selected = validate(input);
  return withFabricSession(
    selected.request,
    selected.scope,
    provider,
    open,
    signal,
    async (model, memory, sessionSignal) => {
      const { data } = await structured(
        model.provider,
        'PERSON_FABRIC memory',
        memoryProfileInstructions,
        { person: selected.person, maxSkills: selected.maxSkills },
        sessionSignal,
        selected.request.model.effort,
        (value) => {
          const context = model.context;
          if (!context) throw new Error('Нет завершённого контекста Fabric.');
          return parseMemoryProfile(value, context, selected.maxSkills);
        },
        'generation',
        true,
      );
      sessionSignal.throwIfAborted();
      const profileSnapshot = decodePersonaProfileSnapshot({
        schemaVersion: 1,
        label: selected.person.label,
        palette: selected.palette,
        archetype: data.rules.length
          ? data.rules
              .map(
                (rule) =>
                  '[' + rule.status + '; ' + rule.evidenceIds.join(', ') + '] ' + rule.instruction,
              )
              .join('\n')
          : 'Недостаточно сведений для характерных черт. Не приписывайте человеку особенности или способности.',
        rules: data.rules,
        sourceStatus: data.rules.some((rule) => rule.status === 'supported')
          ? 'evidence-informed'
          : 'synthetic',
        styleProfile: data.style,
        styleExemplars: data.exemplars,
      });
      const draft = decodePersonaManifestDraft({
        ...selected.draft,
        schemaVersion: 2,
        profileSnapshot,
        contextLeaseIds: model.completedLeaseIds,
        exemplarIds: data.exemplars.map((sample) => sample.id),
        skills: data.skills,
      });
      const references = new Map(
        model.context!.sources.map(({ span }) => {
          const reference = { documentId: span.documentId, revisionId: span.revisionId };
          return [JSON.stringify(reference), reference];
        }),
      );
      const publication = decodeManifestFreeze(
        await memory.freezeManifest(draft, sessionSignal),
        memory.session,
        draft,
        [...references.values()],
      );
      sessionSignal.throwIfAborted();
      const profile = profileFromPublication(publication);
      return { profile, publication, traces: model.traces };
    },
  );
}

/** Shared projection of a validated native publication; never adds task strategies. */
export function profileFromPublication(publication: ManifestFreezeReceipt): PersonProfile {
  if (publication.manifest.schemaVersion !== 2)
    throw new Error('Сохранение не закрепило тело профиля.');
  const stored = publication.manifest.profileSnapshot;
  const profile: PersonProfile = {
    id: publication.manifest.personaId,
    label: stored.label,
    palette: stored.palette,
    archetype: stored.archetype,
    skills: [],
    taskSkills: [],
    rules: stored.rules,
    sourceStatus: stored.sourceStatus,
    sourceContext: true,
    styleProfile: stored.styleProfile,
    styleExemplars: stored.styleExemplars,
    memory: {
      schemaVersion: 1,
      manifestId: publication.manifest.id,
      memorySnapshotId: publication.manifest.memorySnapshotId,
      skills: publication.skills,
    },
  };
  return profile;
}
