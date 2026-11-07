import type { IntelligenceTrace, InvocationAttempt, ProviderId } from '@proteus-int/contracts';
import { IntelligenceRunError } from '@proteus-int/intelligence';
import type { ComponentId, IntelligenceProvider, ModelSettings } from '@proteus-int/intelligence';
import { collectTraces, duration } from './collect-traces';
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
        notify({ id, stage: component, person, label, trace: previous, attempts: [...attempts] });
    };
    const provider: IntelligenceProvider = {
      id: adapter.id,
      async run(request, signal) {
        if (attempts.length) {
          const last = attempts.at(-1)!;
          attempts[attempts.length - 1] = {
            ...last,
            status: request.formatAttempt === 2 ? 'format-rejected' : 'accepted',
          };
          if (request.formatAttempt === 2) emit(`${label} · исправление формата`);
        }
        const started = performance.now();
        try {
          const result = await adapter.run(
            {
              ...request,
              model: settings.aliases[component],
              sourceContext: sourceContext || request.sourceContext === true,
            },
            signal,
          );
          const elapsed = duration(result.trace);
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
          return { ...result, trace: combined };
        } catch (failure) {
          const partial = failure instanceof IntelligenceRunError ? failure.trace : undefined;
          const elapsed = partial ? duration(partial) : Math.max(0, performance.now() - started);
          const message = failure instanceof Error ? failure.message : String(failure);
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
      const last = attempts.at(-1);
      if (last) attempts[attempts.length - 1] = { ...last, status: 'accepted' };
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
      save();
      throw failure;
    }
  }

  return {
    runStage: stage,
    traces,
    get completed() {
      return completed;
    },
  };
}

export type StageRunner = ReturnType<typeof createStageRunner>['runStage'];
