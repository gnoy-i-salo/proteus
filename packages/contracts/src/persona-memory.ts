import type {
  MemoryPort,
  ModelContextLease,
  ModelContextRequest,
  PersonaManifest,
  SkillDefinition,
  SimulationMemoryEntry,
} from './memory';
import type { OpenResearchSessionRequest, ResearchSessionDescriptor } from './research-memory';

export type OpenPersonaSessionRequest = Pick<
  OpenResearchSessionRequest,
  'schemaVersion' | 'operationId' | 'jobId' | 'personaId' | 'period'
> & { readonly manifestId: string };
export interface PersonaSessionDescriptor extends Omit<ResearchSessionDescriptor, 'schemaVersion'> {
  readonly schemaVersion: 2;
  readonly skills: readonly SkillDefinition[];
  readonly purpose: 'personaReply';
  readonly manifest: Extract<PersonaManifest, { readonly schemaVersion: 2 }>;
}
export type PersonaContextRequest = Extract<ModelContextRequest, { purpose: 'personaReply' }>;
export type PersonaContextLease = ModelContextLease & {
  readonly purpose: 'personaReply';
  readonly manifestId: string;
};
// Native checks this proof against the actual completed CLI output. Host scope
// is separate from model JSON; native assigns manifest, role, ID and timestamp.
export interface PersonaSimulationScope {
  readonly experimentId: string;
  readonly trialId: string;
}
export interface AppendPersonaSimulationRequest extends PersonaSimulationScope {
  readonly schemaVersion: 1;
  readonly contextLeaseId: string;
  readonly invocationId: string;
  readonly answer: string;
}
export interface PersonaMemoryPort extends Pick<MemoryPort, 'recall' | 'readEpisode'> {
  readonly session: PersonaSessionDescriptor;
  reserveModelContext(
    request: ModelContextRequest,
    signal?: AbortSignal,
  ): Promise<PersonaContextLease>;
  appendSimulation(
    request: AppendPersonaSimulationRequest,
    signal?: AbortSignal,
  ): Promise<SimulationMemoryEntry>;
  close(): Promise<void>;
}
