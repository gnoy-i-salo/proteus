import type { IntelligenceTrace, InvocationAttempt, ProviderId } from '@proteus-int/contracts';
import { IntelligenceRunError } from '@proteus-int/intelligence';
import type { ComponentId, IntelligenceProvider, ModelSettings } from '@proteus-int/intelligence';
import { collectTraces, duration } from './collect-traces';
import { executionLifecycle } from './execution-lifecycle';
import type { StageCallbacks } from './types';

interface StageInputs {
  readonly settings: ModelSettings;
  readonly providers: Readonly<Record<ProviderId, IntelligenceProvider>>;
  readonly sourceContext: boolean;
  readonly total: number;
}

export function createStageRunner(inputs: StageInputs, options: StageCallbacks) {
  const { settings, providers, sourceContext, total } = inputs;
  let completed = 0;
  const lifecycle = executionLifecycle(options);
  const { traces, notify } = collectTraces(options);
  async function stage<T>(
    id: string,
    component: ComponentId,
    person: number | null,
    label: string,
    run: (provider: IntelligenceProvider) => Promise<T>,
  ): Promise<T> {
    options.signal?.throwIfAborted();
    const adapter = providers[settings.components[component]];
    const attempts: InvocationAttempt[] = [];
    const calls: string[] = [];
    const stageLifecycle = lifecycle.start(id, component, person, label);
    let previous: IntelligenceTrace | undefined;
    const emit = (message: string) =>
      options.onProgress?.({
        id,
        stage: component,
        person,
        completed,
        total,
        message,
      });
    emit(label);
    const save = () => {
      if (previous)
        notify({
          id,
          stage: component,
          person,
          label,
          trace: previous,
          attempts: [...attempts],
          lifecycle: stageLifecycle.current,
        });
    };
    const provider: IntelligenceProvider = {
      id: adapter.id,
      onValidation(event) {
        const callId = event.invocationId ?? calls.at(-1);
        if (!callId) return;
        const index = calls.indexOf(callId);
        if (index < 0 || !attempts[index]) return;
        stageLifecycle.validate(
          callId,
          event.status === 'accepted' ? null : (event.error ?? 'Invalid response format'),
        );
        attempts[index] = {
          ...attempts[index],
          status: event.status === 'accepted' ? 'accepted' : 'format-rejected',
        };
        save();
      },
      async run(request, signal) {
        if (attempts.length) {
          const last = attempts.at(-1)!;
          attempts[attempts.length - 1] = {
            ...last,
            status: request.formatAttempt === 2 ? 'format-rejected' : 'accepted',
          };
          const previousCall = stageLifecycle.current.calls.at(-1);
          if (previousCall?.status === 'validating')
            stageLifecycle.validate(
              previousCall.callId,
              request.formatAttempt === 2 ? 'Response failed format validation' : null,
            );
          if (request.formatAttempt === 2) emit(`${label} · исправление формата`);
        }
        const callId = stageLifecycle.startCall(request);
        calls.push(callId);
        const started = performance.now();
        try {
          const result = await adapter.run(
            {
              ...request,
              invocationId: callId,
              model: settings.aliases[component],
              sourceContext: sourceContext || request.sourceContext === true,
            },
            signal,
          );
          const elapsed = duration(result.trace);
          stageLifecycle.returned(callId, elapsed);
          attempts.push({
            index: attempts.length + 1,
            status: 'returned',
            wallTimeMs: elapsed,
            error: null,
          });
          const combined: IntelligenceTrace = {
            ...result.trace,
            requestedModel: settings.aliases[component],
            requestedEffort: request.effort,
            resolvedModel: result.trace.resolvedModel ?? null,
            appliedEffort: result.trace.appliedEffort ?? null,
            cliVersion: result.trace.cliVersion ?? null,
            activeReasoningMs: null,
            reasoningMs: (previous ? duration(previous) : 0) + elapsed,
            wallTimeMs: (previous ? duration(previous) : 0) + elapsed,
            terminalMs: (previous?.terminalMs ?? 0) + result.trace.terminalMs,
            reasoning: [
              ...(previous?.reasoning ?? []),
              ...(result.trace.reasoning ?? []).map((block) => ({
                ...block,
                id: `${attempts.length}:${block.id}`,
              })),
            ],
            reasoningTruncated: Boolean(
              previous?.reasoningTruncated || result.trace.reasoningTruncated,
            ),
          };
          previous = combined;
          save();
          return { ...result, trace: combined, invocationId: callId };
        } catch (failure) {
          const partial = failure instanceof IntelligenceRunError ? failure.trace : undefined;
          const elapsed = partial ? duration(partial) : Math.max(0, performance.now() - started);
          const message = failure instanceof Error ? failure.message : String(failure);
          stageLifecycle.updateCall(callId, {
            status: signal?.aborted || options.signal?.aborted ? 'cancelled' : 'failed',
            finishedAt: new Date().toISOString(),
            wallTimeMs: elapsed,
            error: message,
          });
          attempts.push({
            index: attempts.length + 1,
            status: 'failed',
            wallTimeMs: elapsed,
            error: message,
          });
          previous = {
            ...partial,
            provider: adapter.id,
            reasoningMs: (previous ? duration(previous) : 0) + elapsed,
            wallTimeMs: (previous ? duration(previous) : 0) + elapsed,
            activeReasoningMs: null,
            requestedModel: settings.aliases[component],
            requestedEffort: request.effort,
            resolvedModel: partial?.resolvedModel ?? null,
            appliedEffort: partial?.appliedEffort ?? null,
            cliVersion: partial?.cliVersion ?? null,
            terminalMs: (previous?.terminalMs ?? 0) + (partial?.terminalMs ?? 0),
            reasoning: [
              ...(previous?.reasoning ?? []),
              ...(partial?.reasoning ?? []).map((block) => ({
                ...block,
                id: `${attempts.length}:${block.id}`,
              })),
            ],
            reasoningTruncated: Boolean(
              previous?.reasoningTruncated || partial?.reasoningTruncated,
            ),
            summary: `Запрос не завершён: ${message}`,
          };
          save();
          throw failure;
        }
      },
    };
    try {
      const result = await run(provider);
      options.signal?.throwIfAborted();
      const last = attempts.at(-1);
      if (last) attempts[attempts.length - 1] = { ...last, status: 'accepted' };
      for (const call of stageLifecycle.current.calls) {
        if (call.status === 'validating') stageLifecycle.validate(call.callId, null);
      }
      stageLifecycle.finish('completed');
      completed++;
      save();
      emit(`${label} · готово`);
      return result;
    } catch (failure) {
      const last = attempts.at(-1);
      if (last?.status === 'returned')
        attempts[attempts.length - 1] = {
          ...last,
          status:
            failure instanceof Error &&
            failure.message.includes('модель дважды вернула неподходящий формат JSON')
              ? 'format-rejected'
              : 'accepted',
        };
      const cancelled = options.signal?.aborted === true;
      const error = failure instanceof Error ? failure.message : String(failure);
      for (const call of stageLifecycle.current.calls) {
        if (call.status === 'validating') {
          if (cancelled)
            stageLifecycle.updateCall(call.callId, {
              status: 'cancelled',
              finishedAt: new Date().toISOString(),
              error,
            });
          else
            stageLifecycle.validate(
              call.callId,
              attempts.at(-1)?.status === 'format-rejected' ? error : null,
            );
        }
      }
      stageLifecycle.finish(cancelled ? 'cancelled' : 'failed', error);
      save();
      throw failure;
    }
  }

  return {
    runStage: stage,
    skipStage: lifecycle.skip,
    snapshot: lifecycle.snapshot,
    traces,
    get completed() {
      return completed;
    },
  };
}

export type StageRunner = ReturnType<typeof createStageRunner>['runStage'];
