export type ProviderId = 'codex' | 'kimi';
export type PaletteId = 'turquoise' | 'violet';
export type IntelligenceEffort = 'low' | 'medium' | 'high';
export interface Evidence {
  readonly id: string;
  readonly 'source-url': string;
  readonly kind: 'fact' | 'hypothesis';
  readonly summary: string;
}
export interface PersonProfile {
  readonly id: string;
  readonly label: string;
  readonly palette: PaletteId;
  readonly archetype: string;
  readonly skills: readonly string[];
}
export interface IntelligenceRequest {
  readonly task: string;
  readonly 'system-prompt': string;
  readonly effort: IntelligenceEffort;
  readonly model?: string;
}
// Text explicitly exposed by the provider's public CLI stream, not encrypted/raw model state.
export interface ReasoningBlock {
  readonly id: string;
  readonly kind: 'summary' | 'thinking';
  readonly text: string;
}
export interface IntelligenceTrace {
  readonly provider: ProviderId;
  readonly 'reasoning-ms': number;
  readonly 'terminal-ms': number;
  readonly summary: string;
  readonly reasoning?: readonly ReasoningBlock[];
  readonly 'reasoning-truncated'?: boolean;
}
export interface IntelligenceResult {
  readonly answer: string;
  readonly trace: IntelligenceTrace;
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
