import type { IntelligenceTrace, InvocationAttempt } from '@proteus-int/contracts';
import type { ComponentId } from '@proteus-int/intelligence';

export interface StageTrace {
  readonly id: string;
  readonly stage: ComponentId;
  readonly person: number | null;
  readonly label: string;
  readonly trace: IntelligenceTrace;
  readonly attempts: readonly InvocationAttempt[];
}
export interface ProgressEvent {
  readonly id: string;
  readonly stage: ComponentId | 'complete';
  readonly person: number | null;
  readonly completed: number;
  readonly total: number;
  readonly message: string;
  readonly trace?: IntelligenceTrace;
}
export interface StageCallbacks {
  readonly signal?: AbortSignal;
  readonly onProgress?: (event: ProgressEvent) => void;
  readonly onTrace?: (trace: StageTrace) => void;
}
