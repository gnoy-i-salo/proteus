import type { IntelligenceEffort, IntelligenceTrace, ProviderId } from '@proteus-int/contracts';
import { duration } from './collect-traces';

interface AttemptTiming {
  readonly requestedModel: string;
  readonly requestedEffort: IntelligenceEffort;
  readonly index: number;
  readonly elapsed: number;
}

export function returnedTrace(
  trace: IntelligenceTrace,
  previous: IntelligenceTrace | undefined,
  attempt: AttemptTiming,
): IntelligenceTrace {
  return {
    ...trace,
    requestedModel: attempt.requestedModel,
    requestedEffort: attempt.requestedEffort,
    resolvedModel: trace.resolvedModel ?? null,
    appliedEffort: trace.appliedEffort ?? null,
    cliVersion: trace.cliVersion ?? null,
    activeReasoningMs: null,
    reasoningMs: (previous ? duration(previous) : 0) + attempt.elapsed,
    wallTimeMs: (previous ? duration(previous) : 0) + attempt.elapsed,
    terminalMs: (previous?.terminalMs ?? 0) + trace.terminalMs,
    reasoning: [
      ...(previous?.reasoning ?? []),
      ...(trace.reasoning ?? []).map((block) => ({
        ...block,
        id: `${attempt.index}:${block.id}`,
      })),
    ],
    reasoningTruncated: Boolean(previous?.reasoningTruncated || trace.reasoningTruncated),
  };
}

export function failedTrace(
  partial: IntelligenceTrace | undefined,
  previous: IntelligenceTrace | undefined,
  provider: ProviderId,
  message: string,
  attempt: AttemptTiming,
): IntelligenceTrace {
  return {
    ...partial,
    provider: provider,
    reasoningMs: (previous ? duration(previous) : 0) + attempt.elapsed,
    wallTimeMs: (previous ? duration(previous) : 0) + attempt.elapsed,
    activeReasoningMs: null,
    requestedModel: attempt.requestedModel,
    requestedEffort: attempt.requestedEffort,
    resolvedModel: partial?.resolvedModel ?? null,
    appliedEffort: partial?.appliedEffort ?? null,
    cliVersion: partial?.cliVersion ?? null,
    terminalMs: (previous?.terminalMs ?? 0) + (partial?.terminalMs ?? 0),
    reasoning: [
      ...(previous?.reasoning ?? []),
      ...(partial?.reasoning ?? []).map((block) => ({
        ...block,
        id: `${attempt.index}:${block.id}`,
      })),
    ],
    reasoningTruncated: Boolean(previous?.reasoningTruncated || partial?.reasoningTruncated),
    summary: `Запрос не завершён: ${message}`,
  };
}
