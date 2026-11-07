import type {
  IntelligenceRequest,
  IntelligenceResult,
  IntelligenceTrace,
  ProviderId,
  InvocationValidation,
} from '@proteus-int/contracts';
export interface IntelligenceProvider {
  readonly id: ProviderId;
  onValidation?(event: InvocationValidation): void;
  run(request: IntelligenceRequest, signal?: AbortSignal): Promise<IntelligenceResult>;
}
export class IntelligenceRunError extends Error {
  readonly trace: IntelligenceTrace;
  constructor(message: string, trace: IntelligenceTrace, options?: ErrorOptions) {
    super(message, options);
    this.name = 'IntelligenceRunError';
    // Detach published snapshots from a still-settling stream or adapter-owned objects.
    this.trace = structuredClone(trace);
  }
}
export function createUnconfiguredProvider(id: ProviderId): IntelligenceProvider {
  return {
    id,
    run() {
      return Promise.reject(
        new Error(
          `Provider ${id} is not configured. Connect a native adapter before running tasks.`,
        ),
      );
    },
  };
}
