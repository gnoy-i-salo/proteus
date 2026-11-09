import { createMemo } from 'solid-js';
import { decodePersonaTaskAdaptation } from '@proteus-int/contracts';
import type {
  EvidenceStatus,
  PersonProfile,
  PersonaTaskAdaptation,
  PersonaRule,
  SkillDefinition,
  StyleExemplar,
} from '@proteus-int/contracts';

// Typed navigation target: it names only references that actually exist in the
// profile. Source evidence is not proof of truth and simulation output is not a
// biography; the callback only hands the reference to the owning screen.
export type PersonaMemoryEvidenceTarget =
  | { readonly kind: 'memoryItem'; readonly memoryItemId: string; readonly skillId: string }
  | {
      readonly kind: 'sourceDocument';
      readonly documentId: string;
      readonly revisionId: string;
      readonly skillId: string;
    }
  | { readonly kind: 'styleExemplar'; readonly exemplarId: string; readonly documentId: string };

export interface PersonaMemorySummary {
  readonly manifestId: string;
  readonly memorySnapshotId: string;
  readonly schemaVersion: number;
}

const evidenceStatusLabels: Record<EvidenceStatus, string> = {
  supported: 'Поддержано источником',
  hypothesis: 'Гипотеза',
  unknown: 'Неизвестно',
  conflicting: 'Противоречие',
};

const skillSupportLabels: Record<SkillDefinition['support'], string> = {
  observed: 'Наблюдаемый',
  proposed: 'Предлагаемый',
};

export function describeEvidenceStatus(status: EvidenceStatus): string {
  return evidenceStatusLabels[status];
}

export function describeSkillSupport(support: SkillDefinition['support']): string {
  return skillSupportLabels[support];
}

export function skillEvidenceTargets(
  skill: SkillDefinition,
): readonly PersonaMemoryEvidenceTarget[] {
  const memoryTargets = skill.memoryItemIds.map((memoryItemId) => ({
    kind: 'memoryItem' as const,
    memoryItemId,
    skillId: skill.id,
  }));
  const documentTargets = skill.sourceSpans.map((span) => ({
    kind: 'sourceDocument' as const,
    documentId: span.documentId,
    revisionId: span.revisionId,
    skillId: skill.id,
  }));
  return [...memoryTargets, ...documentTargets];
}

export function exemplarEvidenceTarget(
  exemplar: StyleExemplar,
): PersonaMemoryEvidenceTarget | null {
  if (!exemplar.documentId) return null;
  return { kind: 'styleExemplar', exemplarId: exemplar.id, documentId: exemplar.documentId };
}

export function createPersonaMemoryViewModel(options: {
  readonly profile: () => PersonProfile;
  readonly baseProfile?: () => PersonProfile | undefined;
  readonly adaptation?: () => PersonaTaskAdaptation | undefined;
  readonly onEvidence?: ((target: PersonaMemoryEvidenceTarget) => void) | undefined;
}) {
  const profile = options.profile;
  const baseProfile = () => options.baseProfile?.() ?? profile();
  const baseRules = () => baseProfile().rules;
  const taskAdaptation = createMemo(() => {
    const supplied = options.adaptation?.();
    if (!supplied) return { value: undefined, error: undefined };
    try {
      const value = decodePersonaTaskAdaptation(supplied, {
        manifestId: profile().memory?.manifestId ?? '',
        task: supplied.task,
        profile: profile(),
      });
      return { value, error: undefined };
    } catch {
      return {
        value: undefined,
        error: 'Адаптация не соответствует сохранённому профилю. Стратегии недоступны.',
      };
    }
  });
  const adaptationRules = () =>
    options.adaptation?.() ? (taskAdaptation().value?.strategies ?? []) : profile().taskSkills;
  const memorySummary = (): PersonaMemorySummary | null => {
    const memory = profile().memory;
    if (!memory) return null;
    return {
      manifestId: memory.manifestId,
      memorySnapshotId: memory.memorySnapshotId,
      schemaVersion: memory.schemaVersion,
    };
  };
  const proceduralSkills = () => profile().memory?.skills ?? [];
  const styleProfile = () => profile().styleProfile ?? null;
  const styleExemplars = () => profile().styleExemplars ?? [];
  const hasMemoryData = () =>
    profile().memory !== undefined ||
    profile().styleProfile !== undefined ||
    (profile().styleExemplars?.length ?? 0) > 0;
  const sourceStatusLabel = () => {
    const status = profile().sourceStatus;
    if (status === 'neutral') return 'Нейтральный контроль';
    if (status === 'synthetic') return 'Синтетический профиль';
    return 'На основе источников';
  };
  const isNeutralControl = () => profile().sourceStatus === 'neutral';
  // Attribution is shown only when the source document reference was actually
  // supplied; nothing is invented to decorate an exemplar.
  const exemplarAttribution = (exemplar: StyleExemplar) =>
    exemplar.documentId ? exemplar.documentId : null;
  const evidenceNavigation = () => (options.onEvidence ? 'available' : 'unavailable');
  const openEvidence = (target: PersonaMemoryEvidenceTarget) => {
    options.onEvidence?.(target);
  };
  return {
    label: () => profile().label,
    palette: () => profile().palette,
    archetype: () => baseProfile().archetype,
    profileSkills: () => baseProfile().skills,
    baseRules,
    adaptationRules,
    adaptationTask: () => taskAdaptation().value?.task,
    adaptationError: () => taskAdaptation().error,
    memorySummary,
    proceduralSkills,
    styleProfile,
    styleExemplars,
    hasMemoryData,
    sourceStatusLabel,
    isNeutralControl,
    exemplarAttribution,
    evidenceNavigation,
    openEvidence,
  };
}

export type PersonaMemoryViewModel = ReturnType<typeof createPersonaMemoryViewModel>;
export type { PersonaRule };
