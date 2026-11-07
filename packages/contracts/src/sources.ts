export type SourceProvider = 'telegram' | 'vk' | 'instagram' | 'github';
export type SourceOperation = 'describe' | 'search' | 'collect' | 'read' | 'coverage';
export interface SourceBudget {
  readonly 'max-steps': number;
  readonly 'max-documents': number;
  readonly 'max-bytes': number;
  readonly 'max-duration-ms': number;
}
export interface SourceGrantRequest {
  readonly provider: SourceProvider;
  readonly 'resource-id': string;
  readonly 'author-id': string | null;
  readonly from: string | null;
  readonly to: string | null;
  readonly 'allow-model-processing': boolean;
  readonly operations: readonly SourceOperation[];
  readonly budget: SourceBudget;
}
// Issued by the native host; possession of a frontend object never grants access.
export interface SourceGrant extends SourceGrantRequest {
  readonly id: string;
  readonly 'account-id': string;
  readonly 'snapshot-id': string;
  readonly 'expires-at': string;
}
export type SourceAction =
  | { readonly action: 'describe' | 'coverage' }
  | { readonly action: 'search'; readonly query: string; readonly limit: number }
  | { readonly action: 'collect'; readonly cursor: string | null; readonly limit: number }
  | { readonly action: 'read'; readonly 'document-ids': readonly string[] };
export type CollectionAction =
  SourceAction | { readonly action: 'finish'; readonly reason: string };
export interface SourceDocument {
  readonly id: string;
  readonly provider: SourceProvider;
  readonly 'resource-id': string;
  readonly 'author-id': string | null;
  readonly authorship: 'owner' | 'other' | 'unknown';
  readonly 'author-label': string | null;
  readonly kind: 'message' | 'post' | 'comment' | 'code' | 'document';
  readonly mode: 'dialogue' | 'comment' | 'formal';
  // Preserve original text and identity; only permitted bounded source context reaches a model.
  readonly text: string;
  readonly 'source-url': string | null;
  readonly 'created-at': string | null;
  readonly 'retrieved-at': string | null;
  readonly 'content-hash': string;
  readonly 'reply-to': string | null;
  readonly 'is-forwarded': boolean;
  readonly truncated: boolean;
  readonly partition: 'reference' | 'holdout';
}
export interface SourceCoverage {
  readonly documents: number;
  readonly 'total-documents': number | null;
  readonly bytes: number;
  readonly complete: boolean;
  readonly truncated: boolean;
  readonly 'not-analyzed': number;
  readonly unavailable: readonly string[];
}
export interface SourceResult {
  readonly 'snapshot-id': string;
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
