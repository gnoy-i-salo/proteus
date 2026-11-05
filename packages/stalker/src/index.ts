import type { Evidence, IntelligenceEffort } from '@proteus-int/contracts';
import type { IntelligenceProvider } from '@proteus-int/intelligence';
export interface StalkerReport {
  readonly evidence: readonly Evidence[];
  readonly 'estimated-bsc': number;
  readonly effort: IntelligenceEffort;
  readonly 'behavior-summary': string;
}
// Public, permitted sources only. Implementations must remove direct identifiers
// before returning a report to Person Fabric.
export interface StalkerPort {
  collect(
    publicUrls: readonly string[],
    provider: IntelligenceProvider,
    signal?: AbortSignal,
  ): Promise<StalkerReport>;
}
