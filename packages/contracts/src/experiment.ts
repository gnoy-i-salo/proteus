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
  readonly 'prompt-version': 'bsc-v2.2-source-style' | 'bsc-v2.3-source-identity';
  readonly 'created-at': string;
  readonly 'source-snapshots'?: readonly [readonly string[], readonly string[]];
}

export interface InvocationAttempt {
  readonly index: number;
  readonly status: 'returned' | 'format-rejected' | 'accepted' | 'failed';
  readonly 'wall-time-ms': number;
  readonly error: string | null;
}
