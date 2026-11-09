import type { MemoryPort, ModelContextLease, ModelContextRequest, PersonaManifest } from './memory';
import type { OpenResearchSessionRequest, ResearchSessionDescriptor } from './research-memory';

export type OpenPersonaSessionRequest = Pick<
  OpenResearchSessionRequest,
  'schemaVersion' | 'operationId' | 'jobId' | 'personaId' | 'period'
> & { readonly manifestId: string };
export interface PersonaSessionDescriptor extends ResearchSessionDescriptor {
  readonly purpose: 'personaReply';
  readonly manifest: PersonaManifest;
}
export type PersonaContextRequest = Extract<ModelContextRequest, { purpose: 'personaReply' }>;
export type PersonaContextLease = ModelContextLease & {
  readonly purpose: 'personaReply';
  readonly manifestId: string;
};
// Host lifecycle and Person recall only. Simulation publication has its own
// completion authority and will be added when that native command is connected.
export interface PersonaMemoryPort extends Pick<MemoryPort, 'recall' | 'readEpisode'> {
  readonly session: PersonaSessionDescriptor;
  reserveModelContext(
    request: ModelContextRequest,
    signal?: AbortSignal,
  ): Promise<PersonaContextLease>;
  close(): Promise<void>;
}
