import type { CorpusCoverage, CorpusSnapshot, SourceBinding } from './corpus';

export interface IngestionBudget {
  readonly maxInputBytes: number;
  readonly maxExpandedBytes: number;
  readonly maxDocuments: number;
  readonly maxBatchDocuments: number;
  readonly maxBatchBytes: number;
  readonly maxDurationMs: number;
}

export type IngestionBindingRequest = Pick<
  SourceBinding,
  | 'personaId'
  | 'role'
  | 'targetAuthorId'
  | 'allowedContextAuthorIds'
  | 'allowUnknownContext'
  | 'period'
> & {
  readonly previewId: string;
  readonly resourceId: string;
  readonly allowModelProcessing: boolean;
  readonly allowExport: boolean;
};

export interface IngestionSelection {
  readonly previewId: string;
  readonly resourceIds: readonly string[];
  readonly bindings: readonly SourceBinding[];
}

export interface IngestionPreview {
  readonly schemaVersion: 1;
  readonly id: string;
  readonly sourceHandle: string;
  readonly sourceFormat: string;
  readonly adapterVersion: string;
  readonly resources: readonly {
    readonly id: string;
    readonly label: string;
    readonly authorIds: readonly string[];
    readonly estimatedDocuments: number | null;
  }[];
  readonly limitations: readonly string[];
}

export interface IngestionJob {
  readonly schemaVersion: 1;
  readonly id: string;
  readonly selection: IngestionSelection;
  readonly status: 'queued' | 'running' | 'paused' | 'cancelled' | 'failed' | 'completed';
  readonly phase: 'reading' | 'normalizing' | 'episodes' | 'indexing' | 'publishing';
  readonly budget: IngestionBudget;
  readonly checkpoint: {
    readonly adapterVersion: string;
    readonly cursor: string | null;
    readonly committedBatch: number;
    readonly stagingGeneration: number;
  };
  readonly coverage: CorpusCoverage;
  readonly publishedSnapshotId: string | null;
  readonly error: {
    readonly code: string;
    readonly message: string;
    readonly retryable: boolean;
  } | null;
}

// sourceHandle is issued by a native picker; never a model-provided filesystem path.
export interface IngestionPort {
  createBinding(request: IngestionBindingRequest, signal?: AbortSignal): Promise<SourceBinding>;
  preview(
    sourceHandle: string,
    budget: IngestionBudget,
    signal?: AbortSignal,
  ): Promise<IngestionPreview>;
  start(
    selection: IngestionSelection,
    budget: IngestionBudget,
    signal?: AbortSignal,
  ): Promise<IngestionJob>;
  getJob(jobId: string, signal?: AbortSignal): Promise<IngestionJob>;
  pause(jobId: string): Promise<IngestionJob>;
  cancel(jobId: string): Promise<IngestionJob>;
  resume(jobId: string, signal?: AbortSignal): Promise<IngestionJob>;
  retry(jobId: string, signal?: AbortSignal): Promise<IngestionJob>;
  // Must reject incomplete/cancelled jobs and atomically publish one generation.
  publish(jobId: string, signal?: AbortSignal): Promise<CorpusSnapshot>;
}

// Local deterministic preparation over a completed, authorized native import.
// The native application owns tokenizer and split policy; no frontend chunks.
export interface CorpusPreparationPort {
  prepare(jobId: string, maxDurationMs: number, signal?: AbortSignal): Promise<CorpusSnapshot>;
}
