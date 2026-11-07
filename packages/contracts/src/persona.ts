import type { PersonaMemoryProfile } from './memory';
import type { PersonaStyleProfile, StyleExemplar } from './style';

export type EvidenceStatus = 'supported' | 'hypothesis' | 'unknown' | 'conflicting';
export interface Evidence {
  readonly id: string;
  readonly sourceUrl: string;
  readonly kind: 'fact' | 'hypothesis';
  readonly summary: string;
  readonly status: EvidenceStatus;
  readonly verification: 'model-reported' | 'source-retrieved' | 'unverified';
  readonly documentIds?: readonly string[];
  readonly excerpt: string | null;
  readonly retrievedAt: string | null;
  readonly limitations: readonly string[];
}
export interface PersonaRule {
  readonly id: string;
  readonly instruction: string;
  readonly evidenceIds: readonly string[];
  readonly status: EvidenceStatus;
  readonly limitations: readonly string[];
}
export interface PersonProfile {
  readonly id: string;
  readonly label: string;
  readonly palette: PaletteId;
  readonly archetype: string;
  readonly skills: readonly string[];
  readonly rules: readonly PersonaRule[];
  readonly taskSkills: readonly PersonaRule[];
  readonly sourceStatus: 'evidence-informed' | 'synthetic' | 'neutral';
  readonly sourceContext?: boolean;
  // Absent on existing non-RAG profiles; the nested extension carries its own version.
  readonly memory?: PersonaMemoryProfile;
  readonly styleProfile?: PersonaStyleProfile;
  readonly styleExemplars?: readonly StyleExemplar[];
}

export type PaletteId = 'turquoise' | 'violet';
export interface PersonInput {
  readonly label: string;
  readonly context: string;
  readonly publicUrls: readonly string[];
}
export interface PersonaObservation {
  readonly strengths: readonly string[];
  readonly weaknesses: readonly string[];
  readonly observations: readonly string[];
}
