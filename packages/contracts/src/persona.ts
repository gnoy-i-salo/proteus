import type { PaletteId } from './index';
import type { PersonaStyleProfile, StyleExemplar } from './style';

export type EvidenceStatus = 'supported' | 'hypothesis' | 'unknown' | 'conflicting';
export interface Evidence {
  readonly id: string;
  readonly 'source-url': string;
  readonly kind: 'fact' | 'hypothesis';
  readonly summary: string;
  readonly status: EvidenceStatus;
  readonly verification: 'model-reported' | 'source-retrieved' | 'unverified';
  readonly 'document-ids'?: readonly string[];
  readonly excerpt: string | null;
  readonly 'retrieved-at': string | null;
  readonly limitations: readonly string[];
}
export interface PersonaRule {
  readonly id: string;
  readonly instruction: string;
  readonly 'evidence-ids': readonly string[];
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
  readonly 'task-skills': readonly PersonaRule[];
  readonly 'source-status': 'evidence-informed' | 'synthetic' | 'neutral';
  readonly 'source-context'?: boolean;
  readonly 'style-profile'?: PersonaStyleProfile;
  readonly 'style-exemplars'?: readonly StyleExemplar[];
}
