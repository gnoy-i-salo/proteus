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
}
// Observable timing/output summaries only; never private model chain-of-thought.
export interface IntelligenceTrace {
  readonly provider: ProviderId;
  readonly 'reasoning-ms': number;
  readonly 'terminal-ms': number;
  readonly summary: string;
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
