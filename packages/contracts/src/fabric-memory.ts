import type { MemoryPort, ModelContextLease } from './memory';
import type { OpenResearchSessionRequest, ResearchSessionDescriptor } from './research-memory';

export type OpenFabricSessionRequest = Omit<OpenResearchSessionRequest, 'mode'> & {
  readonly mode: 'lexical';
};
export type FabricSessionDescriptor = Omit<ResearchSessionDescriptor, 'mode'> & {
  readonly mode: 'lexical';
  readonly purpose: 'fabric';
};
export interface FabricContextRequest {
  readonly schemaVersion: 1;
  readonly memorySnapshotId: string;
  readonly itemIds: readonly string[];
  readonly systemPrompt: string;
  readonly task: string;
  readonly formatAttempt: 1 | 2;
}
export type FabricContextLease = Omit<ModelContextLease, 'purpose' | 'manifestId'> & {
  readonly purpose: 'fabric';
  readonly manifestId: null;
};

// Preparation and freeze only; native owns completion proof and source authority.
export interface FabricMemoryPort extends Pick<MemoryPort, 'freezeManifest'> {
  readonly session: FabricSessionDescriptor;
  reserveModelContext(
    request: FabricContextRequest,
    signal?: AbortSignal,
  ): Promise<FabricContextLease>;
  close(): Promise<void>;
}
