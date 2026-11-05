import type { IntelligenceRequest, IntelligenceResult, ProviderId } from '@proteus-int/contracts';
export interface IntelligenceProvider {
  readonly id: ProviderId;
  run(request: IntelligenceRequest, signal?: AbortSignal): Promise<IntelligenceResult>;
}
export function createUnconfiguredProvider(id: ProviderId): IntelligenceProvider {
  return {
    id,
    async run() {
      throw new Error(
        `Provider ${id} is not configured. Connect a native adapter before running tasks.`,
      );
    },
  };
}
