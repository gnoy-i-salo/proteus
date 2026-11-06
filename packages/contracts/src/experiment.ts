import type { EvaluationRubric } from './evaluation';
import type { IntelligenceEffort, ProviderId } from './index';

export interface ExperimentProtocol {
  readonly version: 2;
  readonly task: string;
  readonly rubric: EvaluationRubric;
  readonly repetitions: 1 | 3;
  readonly 'participant-order': readonly (readonly string[])[];
  readonly 'model-settings': Readonly<
    Record<
      string,
      {
        readonly provider: ProviderId;
        readonly 'requested-model': string;
        readonly 'requested-effort': IntelligenceEffort;
      }
    >
  >;
  readonly 'format-attempts': 2;
  readonly 'stage-timeout-ms': 600000;
  readonly 'prompt-version': 'bsc-v2.1';
  readonly 'created-at': string;
}

export interface InvocationAttempt {
  readonly index: number;
  readonly status: 'returned' | 'format-rejected' | 'accepted' | 'failed';
  readonly 'wall-time-ms': number;
  readonly error: string | null;
}
