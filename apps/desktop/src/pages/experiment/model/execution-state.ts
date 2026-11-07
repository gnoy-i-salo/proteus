import { createSignal, onCleanup } from 'solid-js';
import type { StageLifecycle } from '@proteus-int/contracts';

export type RunStatus =
  | 'idle'
  | 'preparing'
  | 'running'
  | 'waitingForProtocol'
  | 'stopping'
  | 'completed'
  | 'incomplete'
  | 'failed'
  | 'cancelled';
const isActive = (stage: StageLifecycle) =>
  ['running', 'validating', 'formatRetry'].includes(stage.status);
export function createExecutionState() {
  const [execution, setExecution] = createSignal<readonly StageLifecycle[]>([]);
  const [runStatus, setRunStatus] = createSignal<RunStatus>('idle');
  const [elapsedMs, setElapsedMs] = createSignal(0);
  const [lastCliEventAt, setLastCliEventAt] = createSignal<number | null>(null);
  const [failureReason, setFailureReason] = createSignal('');
  let startedAt = 0;
  let timer: ReturnType<typeof setInterval> | undefined;
  const updateClock = () => setElapsedMs(Math.max(0, performance.now() - startedAt));
  const stopClock = () => {
    if (timer === undefined) return;
    updateClock();
    clearInterval(timer);
    timer = undefined;
  };
  onCleanup(stopClock);
  const activeStage = () => [...execution()].reverse().find(isActive);
  const failedStage = () =>
    [...execution()]
      .reverse()
      .find((stage) => stage.status === 'failed' || stage.status === 'cancelled');
  return {
    execution,
    runStatus,
    elapsedMs,
    failureReason,
    activeStageId: () => activeStage()?.logicalStageId,
    failedStageId: () => failedStage()?.logicalStageId,
    displayStage: () =>
      activeStage() ??
      failedStage() ??
      [...execution()].reverse().find((stage) => stage.status !== 'skipped'),
    lastCliEventAgeMs: () => {
      const received = lastCliEventAt();
      return received === null ? null : Math.max(0, elapsedMs() - received);
    },
    begin() {
      stopClock();
      startedAt = performance.now();
      setExecution([]);
      setElapsedMs(0);
      setLastCliEventAt(null);
      setFailureReason('');
      setRunStatus('preparing');
      timer = setInterval(updateClock, 250);
    },
    setRunStatus,
    record(stage: StageLifecycle) {
      setExecution((items) =>
        items.some((item) => item.stageId === stage.stageId)
          ? items.map((item) => (item.stageId === stage.stageId ? stage : item))
          : [...items, stage],
      );
    },
    receive(callId: string) {
      const stage = execution().find((item) => item.calls.some((call) => call.callId === callId));
      const index = stage?.calls.findIndex((call) => call.callId === callId) ?? -1;
      const call = stage?.calls[index];
      if (!stage || !isActive(stage) || !call || !['running', 'validating'].includes(call.status))
        return undefined;
      updateClock();
      setLastCliEventAt(elapsedMs());
      return { stage, callIndex: index + 1 };
    },
    finish(status: RunStatus, reason = '') {
      stopClock();
      setRunStatus(status);
      setFailureReason(reason);
    },
  };
}
