import type { PersonReply } from './style';

export type ProviderId = 'codex' | 'kimi';
export type PaletteId = 'turquoise' | 'violet';
export type IntelligenceEffort = 'low' | 'medium' | 'high';
export * from './persona';
export * from './evaluation';
export * from './experiment';
export * from './auth-callback';
export * from './connections';
export * from './sources';
export {
  decodeLegacySourceDocument,
  decodeStoredSourceDocument,
  encodeStoredSourceDocument,
} from './source-document-codec';
export type { StoredSourceDocument } from './source-document-codec';
export * from './style';
export type IntelligenceOperation =
  'generation' | 'public-research' | 'source-plan' | 'corpus-analysis' | 'persona-reply';
export interface IntelligenceRequest {
  readonly 'source-context'?: boolean;
  readonly task: string;
  readonly 'system-prompt': string;
  readonly effort: IntelligenceEffort;
  readonly model?: string;
  readonly operation?: IntelligenceOperation;
  readonly 'format-attempt'?: 1 | 2;
}
// Text explicitly exposed by the provider's public CLI stream, not encrypted/raw model state.
export interface ReasoningBlock {
  readonly id: string;
  readonly kind: 'summary' | 'thinking';
  readonly text: string;
}
export interface IntelligenceTrace {
  readonly provider: ProviderId;
  /** Legacy field: elapsed request duration, never measured active reasoning. */
  readonly 'reasoning-ms': number;
  readonly 'wall-time-ms'?: number;
  readonly 'active-reasoning-ms'?: null;
  readonly 'requested-model'?: string;
  readonly 'resolved-model'?: string | null;
  readonly 'requested-effort'?: IntelligenceEffort;
  readonly 'applied-effort'?: IntelligenceEffort | null;
  readonly 'cli-version'?: string | null;
  readonly 'terminal-ms': number;
  readonly summary: string;
  readonly reasoning?: readonly ReasoningBlock[];
  readonly 'reasoning-truncated'?: boolean;
}
export interface IntelligenceResult {
  readonly answer: string;
  readonly trace: IntelligenceTrace;
  readonly reply?: PersonReply;
}
export interface PersonScore {
  readonly 'person-id': string;
  readonly 'reasoning-minutes': number;
  readonly bsc: number;
}

export interface PersonInput {
  readonly label: string;
  readonly context: string;
  readonly 'public-urls': readonly string[];
}
export interface ExperimentInput {
  readonly task: string;
  readonly people: readonly [PersonInput, PersonInput];
}
export interface PersonaObservation {
  readonly strengths: readonly string[];
  readonly weaknesses: readonly string[];
  readonly observations: readonly string[];
}
