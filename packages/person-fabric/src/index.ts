import type { PersonProfile } from '@proteus-int/contracts';
import type { IntelligenceProvider } from '@proteus-int/intelligence';
import type { PersonPort } from '@proteus-int/person';
import type { StalkerReport } from '@proteus-int/stalker';
export interface FabricatedPerson {
  readonly profile: PersonProfile;
  readonly runtime: PersonPort;
}
export interface PersonFabricPort {
  create(
    report: StalkerReport,
    task: string,
    provider: IntelligenceProvider,
    signal?: AbortSignal,
  ): Promise<readonly FabricatedPerson[]>;
}
