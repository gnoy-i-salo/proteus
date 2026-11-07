import type {
  ExecutionSnapshot,
  ExecutionStatus,
  IntelligenceRequest,
  ModelCallLifecycle,
  StageLifecycle,
} from '@proteus-int/contracts';
import type { StageCallbacks } from './types';

export function executionLifecycle(options: Pick<StageCallbacks, 'onExecution'>) {
  const runId = crypto.randomUUID();
  const stages: StageLifecycle[] = [];
  function publish(value: StageLifecycle) {
    const index = stages.findIndex((stage) => stage.stageId === value.stageId);
    if (index < 0) stages.push(value);
    else stages[index] = value;
    options.onExecution?.(structuredClone(value));
  }
  function start(logicalStageId: string, component: string, person: number | null, label: string) {
    let value: StageLifecycle = {
      schemaVersion: 1,
      runId,
      stageId: crypto.randomUUID(),
      logicalStageId,
      component,
      person,
      label,
      status: 'running',
      startedAt: new Date().toISOString(),
      finishedAt: null,
      error: null,
      calls: [],
    };
    publish(value);
    function change(update: Partial<StageLifecycle>) {
      value = { ...value, ...update };
      publish(value);
    }
    function updateCall(callId: string, update: Partial<ModelCallLifecycle>) {
      change({
        calls: value.calls.map((call) => (call.callId === callId ? { ...call, ...update } : call)),
      });
    }
    function startCall(request: IntelligenceRequest) {
      const call: ModelCallLifecycle = {
        callId: crypto.randomUUID(),
        operation: request.operation ?? 'generation',
        trigger: request.formatAttempt === 2 ? 'formatRepair' : 'initial',
        formatAttempt: request.formatAttempt ?? 1,
        status: 'running',
        startedAt: new Date().toISOString(),
        returnedAt: null,
        finishedAt: null,
        wallTimeMs: null,
        error: null,
        validationError: null,
      };
      change({
        status: call.trigger === 'formatRepair' ? 'formatRetry' : 'running',
        calls: [...value.calls, call],
      });
      return call.callId;
    }
    function returned(callId: string, wallTimeMs: number) {
      change({ status: 'validating' });
      updateCall(callId, {
        status: 'validating',
        returnedAt: new Date().toISOString(),
        wallTimeMs,
      });
    }
    function validate(callId: string, error: string | null) {
      updateCall(callId, {
        status: error === null ? 'completed' : 'failed',
        validationError: error,
        finishedAt: new Date().toISOString(),
      });
    }
    function finish(
      status: Extract<ExecutionStatus, 'completed' | 'failed' | 'cancelled'>,
      error: string | null = null,
    ) {
      change({ status, error, finishedAt: new Date().toISOString() });
    }
    return {
      startCall,
      updateCall,
      returned,
      validate,
      finish,
      get current() {
        return structuredClone(value);
      },
    };
  }
  function skip(
    logicalStageId: string,
    component: string,
    person: number | null,
    label: string,
    error: string,
  ) {
    if (stages.some((stage) => stage.logicalStageId === logicalStageId)) return;
    publish({
      schemaVersion: 1,
      runId,
      stageId: crypto.randomUUID(),
      logicalStageId,
      component,
      person,
      label,
      status: 'skipped',
      startedAt: null,
      finishedAt: new Date().toISOString(),
      error,
      calls: [],
    });
  }
  function snapshot(): ExecutionSnapshot {
    return structuredClone({ schemaVersion: 1, runId, stages });
  }
  return { start, skip, snapshot };
}
