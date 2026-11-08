import type { PersonReply } from './style';

export type ProviderId = 'codex' | 'kimi';
export type IntelligenceEffort = 'low' | 'medium' | 'high';
export type IntelligenceOperation =
  'generation' | 'public-research' | 'source-plan' | 'corpus-analysis' | 'persona-reply';
export interface ModelContextReference {
  readonly sessionId: string;
  readonly leaseId: string;
}
export interface IntelligenceRequest {
  /** Host-assigned invocation identity; adapters use it for stream correlation. */
  readonly invocationId?: string;
  readonly sourceContext?: boolean;
  readonly memoryContext?: ModelContextReference;
  readonly task: string;
  readonly systemPrompt: string;
  readonly effort: IntelligenceEffort;
  readonly model?: string;
  readonly operation?: IntelligenceOperation;
  readonly formatAttempt?: 1 | 2;
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
  readonly reasoningMs: number;
  readonly wallTimeMs?: number;
  readonly activeReasoningMs?: null;
  readonly requestedModel?: string;
  readonly resolvedModel?: string | null;
  readonly requestedEffort?: IntelligenceEffort;
  readonly appliedEffort?: IntelligenceEffort | null;
  readonly cliVersion?: string | null;
  readonly terminalMs: number;
  readonly summary: string;
  readonly reasoning?: readonly ReasoningBlock[];
  readonly reasoningTruncated?: boolean;
}
export interface IntelligenceResult {
  readonly invocationId?: string;
  readonly answer: string;
  readonly trace: IntelligenceTrace;
  readonly reply?: PersonReply;
}
