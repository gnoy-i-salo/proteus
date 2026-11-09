import type { IntelligenceEffort, ProviderId } from './intelligence';
import type { MemoryPort, ModelCallBudget, RecallBudget } from './memory';

export interface OpenResearchSessionRequest {
  readonly schemaVersion: 1;
  readonly operationId: string;
  readonly jobId: string;
  readonly snapshotId: string;
  readonly personaId: string;
  readonly mode: 'lexical' | 'hybrid';
  readonly period: {
    readonly from: number | null;
    readonly to: number | null;
    readonly includeUndated: boolean;
  };
  readonly recallBudget: RecallBudget;
  readonly modelBudget: ModelCallBudget;
  readonly model: {
    readonly provider: ProviderId;
    readonly model: string;
    readonly effort: IntelligenceEffort;
  };
}

export interface ResearchSessionDescriptor {
  readonly schemaVersion: 1;
  readonly id: string;
  readonly personaId: string;
  readonly corpusSnapshotId: string;
  readonly policyVersion: string;
  readonly mode: 'lexical' | 'hybrid';
  readonly indexGeneration: number;
  readonly expiresAt: string;
}

// Host lifecycle and research operations only; manifests and simulation belong
// to their separate native authority, not fabricated methods on this port.
export interface ResearchMemoryPort extends Pick<
  MemoryPort,
  'recall' | 'readEpisode' | 'publishResearch' | 'reserveModelContext'
> {
  readonly session: ResearchSessionDescriptor;
  close(): Promise<void>;
}
