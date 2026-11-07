import type { IntelligenceResult, PersonProfile } from '@proteus-int/contracts';
import type { IntelligenceProvider } from '@proteus-int/intelligence';
export interface PersonPort {
  solve(
    profile: PersonProfile,
    task: string,
    provider: IntelligenceProvider,
    signal?: AbortSignal,
  ): Promise<IntelligenceResult>;
}
