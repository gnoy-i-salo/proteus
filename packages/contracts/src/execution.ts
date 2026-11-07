import type { IntelligenceOperation } from './intelligence';

export type ExecutionStatus =
  'running' | 'validating' | 'formatRetry' | 'completed' | 'failed' | 'cancelled' | 'skipped';

export interface ModelCallLifecycle {
  readonly callId: string;
  readonly operation: IntelligenceOperation;
  readonly trigger: 'initial' | 'formatRepair';
  readonly formatAttempt: 1 | 2;
  readonly status: Exclude<ExecutionStatus, 'formatRetry' | 'skipped'>;
  readonly startedAt: string;
  readonly returnedAt: string | null;
  readonly finishedAt: string | null;
  readonly wallTimeMs: number | null;
  readonly error: string | null;
  readonly validationError: string | null;
}

export interface StageLifecycle {
  readonly schemaVersion: 1;
  readonly runId: string;
  readonly stageId: string;
  readonly logicalStageId: string;
  readonly component: string;
  readonly person: number | null;
  readonly label: string;
  readonly status: ExecutionStatus;
  readonly startedAt: string | null;
  readonly finishedAt: string | null;
  readonly error: string | null;
  readonly calls: readonly ModelCallLifecycle[];
}

export interface ExecutionSnapshot {
  readonly schemaVersion: 1;
  readonly runId: string;
  readonly stages: readonly StageLifecycle[];
}

/** Validation is a host observation; it is never sent to a model as a tool. */
export interface InvocationValidation {
  readonly invocationId?: string;
  readonly status: 'accepted' | 'rejected';
  readonly error: string | null;
}
