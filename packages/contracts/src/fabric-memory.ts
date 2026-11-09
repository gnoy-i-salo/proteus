import type { ModelContextLease } from './memory';
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

// Published-memory preparation only. Native freeze has its own completion proof
// and will extend this port when that command exists, rather than return a fake result.
export interface FabricMemoryPort {
  readonly session: FabricSessionDescriptor;
  reserveModelContext(
    request: FabricContextRequest,
    signal?: AbortSignal,
  ): Promise<FabricContextLease>;
  close(): Promise<void>;
}
