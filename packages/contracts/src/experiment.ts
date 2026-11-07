import type { ExperimentMemoryProtocol } from './memory';
import type { PersonInput } from './persona';
import type { EvaluationRubric } from './evaluation';
import type { IntelligenceEffort, ProviderId } from './intelligence';

export interface ExperimentProtocol {
  readonly version: 3;
  readonly task: string;
  readonly rubric: EvaluationRubric;
  readonly repetitions: 1 | 3;
  readonly participantOrder: readonly (readonly string[])[];
  readonly modelSettings: Readonly<
    Record<
      string,
      {
        readonly provider: ProviderId;
        readonly requestedModel: string;
        readonly requestedEffort: IntelligenceEffort;
      }
    >
  >;
  readonly formatAttempts: 2;
  readonly stageTimeoutMs: 600000;
  readonly promptVersion:
    'bsc-v2.2-source-style' | 'bsc-v2.3-source-identity' | 'bsc-v2.4-camel-case';
  readonly createdAt: string;
  // Absence preserves the existing fixed non-RAG protocol behavior.
  readonly memory?: ExperimentMemoryProtocol;
  readonly sourceSnapshots?: readonly [readonly string[], readonly string[]];
}

export interface InvocationAttempt {
  readonly index: number;
  readonly status: 'returned' | 'format-rejected' | 'accepted' | 'failed';
  readonly wallTimeMs: number;
  readonly error: string | null;
}

export interface ExperimentInput {
  readonly task: string;
  readonly people: readonly [PersonInput, PersonInput];
}
