export type SourceProvider = 'telegram' | 'vk' | 'instagram' | 'github';
export type SourceOperation = 'describe' | 'search' | 'collect' | 'read' | 'coverage';
export interface SourceBudget {
  readonly maxSteps: number;
  readonly maxDocuments: number;
  readonly maxBytes: number;
  readonly maxDurationMs: number;
}
export interface SourceGrantRequest {
  readonly provider: SourceProvider;
  readonly resourceId: string;
  readonly authorId: string | null;
  readonly from: string | null;
  readonly to: string | null;
  readonly allowModelProcessing: boolean;
  readonly operations: readonly SourceOperation[];
  readonly budget: SourceBudget;
}
// Issued by the native host; possession of a frontend object never grants access.
export interface SourceGrant extends SourceGrantRequest {
  readonly id: string;
  readonly accountId: string;
  readonly snapshotId: string;
  readonly expiresAt: string;
}
export type SourceAction =
  | { readonly action: 'describe' | 'coverage' }
  | { readonly action: 'search'; readonly query: string; readonly limit: number }
  | { readonly action: 'collect'; readonly cursor: string | null; readonly limit: number }
  | { readonly action: 'read'; readonly documentIds: readonly string[] };
export type CollectionAction =
  SourceAction | { readonly action: 'finish'; readonly reason: string };
export interface SourceDocument {
  readonly id: string;
  readonly provider: SourceProvider;
  readonly resourceId: string;
  readonly authorId: string | null;
  readonly authorship: 'owner' | 'other' | 'unknown';
  readonly authorLabel: string | null;
  readonly kind: 'message' | 'post' | 'comment' | 'code' | 'document';
  readonly mode: 'dialogue' | 'comment' | 'formal';
  // Preserve original text and identity; only permitted bounded source context reaches a model.
  readonly text: string;
  readonly sourceUrl: string | null;
  readonly createdAt: string | null;
  readonly retrievedAt: string | null;
  readonly contentHash: string;
  readonly replyTo: string | null;
  readonly isForwarded: boolean;
  readonly truncated: boolean;
  readonly partition: 'reference' | 'holdout';
}
export interface SourceCoverage {
  readonly documents: number;
  readonly totalDocuments: number | null;
  readonly bytes: number;
  readonly complete: boolean;
  readonly truncated: boolean;
  readonly notAnalyzed: number;
  readonly unavailable: readonly string[];
}
export interface SourceResult {
  readonly snapshotId: string;
  readonly documents: readonly SourceDocument[];
  // Cumulative native coverage across pages/retries, never a model estimate.
  readonly coverage: SourceCoverage;
  readonly cursor: string | null;
  readonly limitations: readonly string[];
}
export interface SourcePort {
  execute(grantId: string, action: SourceAction, signal?: AbortSignal): Promise<SourceResult>;
  reserveModelCall(grantId: string, signal?: AbortSignal): Promise<void>;
}
