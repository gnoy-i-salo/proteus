import type { StoredSourceDocument } from './source-document-codec';
import type { SourceProvider } from './sources';
import { validateRevisionProvenance } from './document-provenance';

export type CorpusProvider = SourceProvider | 'web' | 'savedPage';
export type SourceRole = 'own' | 'context' | 'about' | 'reference';

export interface SourceBinding {
  readonly schemaVersion: 1;
  readonly id: string;
  readonly personaId: string;
  readonly provider: CorpusProvider;
  readonly accountId: string | null;
  readonly resourceId: string;
  readonly grantId: string;
  readonly policyVersion: string;
  readonly role: SourceRole;
  readonly targetAuthorId: string | null;
  readonly allowedContextAuthorIds: readonly string[];
  readonly allowUnknownContext: boolean;
  readonly period: {
    readonly from: string | null;
    readonly to: string | null;
    readonly includeUndated: boolean;
  };
}

export interface RevisionReference {
  readonly documentId: string;
  readonly revisionId: string;
}

// Offsets refer to the exact immutable original bytes, never normalized text.
export interface SourceSpan extends RevisionReference {
  readonly unit: 'utf8Bytes';
  readonly start: number;
  readonly end: number;
}

export interface SourceAuthor {
  readonly provider: CorpusProvider | null;
  readonly id: string | null;
  readonly label: string | null;
}

export interface DocumentRelation {
  readonly kind: 'reply' | 'quote' | 'forward';
  readonly original: RevisionReference | null;
  readonly originalAuthor: SourceAuthor;
  readonly span: SourceSpan | null;
}

export interface AttachmentMetadata {
  readonly id: string;
  readonly kind: 'image' | 'audio' | 'video' | 'file' | 'link';
  readonly name: string | null;
  readonly mediaType: string | null;
  readonly byteLength: number | null;
  readonly sourceUrl: string | null;
  readonly availability: 'available' | 'missing' | 'unsupported';
}

export type ProvenanceLimitation =
  | 'originalObjectIdUnavailable'
  | 'originalBytesUnavailable'
  | 'attachmentsUnavailable'
  | 'relationsIncomplete';

export interface DocumentRevision {
  readonly schemaVersion: 1;
  readonly revisionId: string;
  readonly sourceKey: string;
  readonly accountId: string | null;
  // Reuse the canonical compatibility shape while adding web acquisition.
  readonly document: Omit<StoredSourceDocument, 'provider'> & {
    readonly provider: CorpusProvider;
  };
  readonly editedAt: string | null;
  readonly relations: readonly DocumentRelation[];
  readonly attachments: readonly AttachmentMetadata[];
  readonly provenance: {
    readonly acquisition: 'api' | 'archive' | 'selectedUrl' | 'savedPage';
    readonly adapterVersion: string;
    readonly sourceFormat: string;
    readonly originalObjectId: string | null;
    readonly originalByteLength: number | null;
    readonly hashAlgorithm: 'sha256';
    readonly limitations: readonly ProvenanceLimitation[];
  };
  readonly usagePolicy: {
    readonly grantId: string;
    readonly version: string;
    readonly allowModelProcessing: boolean;
    readonly allowExport: boolean;
  };
}

export interface Episode {
  readonly schemaVersion: 1;
  readonly id: string;
  readonly snapshotId: string;
  readonly bindingId: string;
  readonly conversationId: string;
  readonly threadId: string | null;
  readonly documentRefs: readonly RevisionReference[];
  readonly partition: 'reference' | 'holdout';
  readonly splitVersion: string;
  readonly assemblyVersion: string;
  readonly limitations: readonly string[];
}

export interface SearchChunk {
  readonly schemaVersion: 1;
  readonly id: string;
  readonly episodeId: string;
  readonly snapshotId: string;
  readonly spans: readonly SourceSpan[];
  readonly text: string;
  readonly tokenCount: number;
  readonly tokenizerVersion: string;
  readonly chunkingVersion: string;
  readonly partition: 'reference';
}

export interface CorpusCoverage {
  readonly loadedDocuments: number;
  readonly indexedDocuments: number;
  readonly analyzedDocuments: number;
  readonly usedDocuments: number;
  readonly totalDocuments: number | null;
  readonly loadedBytes: number;
  readonly complete: boolean;
  readonly limitations: readonly string[];
}

export interface CorpusSnapshot {
  readonly schemaVersion: 1;
  readonly id: string;
  readonly corpusId: string;
  readonly parentSnapshotId: string | null;
  readonly createdAt: string;
  readonly bindingIds: readonly string[];
  readonly documentRefs: readonly RevisionReference[];
  readonly episodeIds: readonly string[];
  readonly splitVersion: string;
  readonly generation: number;
  readonly coverage: CorpusCoverage;
}

export interface CorpusReadBudget {
  readonly maxDocuments: number;
  readonly maxBytes: number;
  readonly maxDurationMs: number;
}

// Native implementation rechecks current permissions, including old snapshots.
export interface CorpusPort {
  getSnapshot(snapshotId: string, signal?: AbortSignal): Promise<CorpusSnapshot>;
  readRevisions(
    snapshotId: string,
    refs: readonly RevisionReference[],
    budget: CorpusReadBudget,
    signal?: AbortSignal,
  ): Promise<readonly DocumentRevision[]>;
  coverage(snapshotId: string, signal?: AbortSignal): Promise<CorpusCoverage>;
}

export interface ModelSourceDocument {
  readonly document: DocumentRevision['document'];
  readonly revisionId: string;
  readonly role: SourceRole;
  readonly speakerRelation: 'target' | 'context' | 'unknown';
  // Eligibility for attributed extraction, never confirmation that a claim is true.
  readonly canSupportOwnWords: boolean;
  readonly relations: readonly DocumentRelation[];
}

// Exact bounded text with the existing attribution projection; full document text
// and its duplicate revisionId are omitted. The span is the sole revision reference.
export type SourceExcerpt = Omit<ModelSourceDocument, 'document' | 'revisionId'> & {
  readonly span: SourceSpan;
  readonly text: string;
  readonly document: Omit<ModelSourceDocument['document'], 'text'>;
};

// Structural projection only: never an authorization token or a substitute for
// native scope, expiry and revocation checks immediately before model dispatch.
export function projectBoundRevision(
  binding: SourceBinding,
  revision: DocumentRevision,
): ModelSourceDocument {
  validateRevisionProvenance(revision);
  const document = revision.document;
  if (
    document.provider !== binding.provider ||
    revision.accountId !== binding.accountId ||
    document.resourceId !== binding.resourceId ||
    revision.usagePolicy.grantId !== binding.grantId ||
    revision.usagePolicy.version !== binding.policyVersion ||
    !revision.usagePolicy.allowModelProcessing ||
    document.partition !== 'reference'
  )
    throw new Error('Документ вне выбранной области модельной обработки.');
  validatePeriod(binding.period, document.createdAt);
  const speakerRelation = authorRelation(binding, document.authorId);
  const forwarded =
    document.isForwarded || revision.relations.some((item) => item.kind === 'forward');
  const quoted = revision.relations.some((item) => item.kind === 'quote');
  // A whole quoted/forwarded document cannot establish the target's own words.
  // Later extraction may use attributed non-quoted spans, never an entire quote.
  const canSupportOwnWords =
    document.authorship === 'owner' &&
    speakerRelation === 'target' &&
    binding.role === 'own' &&
    !forwarded &&
    !quoted;
  return {
    document: {
      id: document.id,
      provider: document.provider,
      resourceId: document.resourceId,
      authorId: document.authorId,
      authorLabel: document.authorLabel,
      authorship: document.authorship,
      kind: document.kind,
      mode: document.mode,
      text: document.text,
      sourceUrl: document.sourceUrl,
      createdAt: document.createdAt,
      retrievedAt: document.retrievedAt,
      contentHash: document.contentHash,
      replyTo: document.replyTo,
      isForwarded: document.isForwarded,
      truncated: document.truncated,
      partition: document.partition,
    },
    revisionId: revision.revisionId,
    role: binding.role,
    speakerRelation,
    canSupportOwnWords,
    relations: revision.relations.map((item) => ({
      kind: item.kind,
      original:
        item.original === null
          ? null
          : {
              documentId: item.original.documentId,
              revisionId: item.original.revisionId,
            },
      originalAuthor: {
        provider: item.originalAuthor.provider,
        id: item.originalAuthor.id,
        label: item.originalAuthor.label,
      },
      span:
        item.span === null
          ? null
          : {
              documentId: item.span.documentId,
              revisionId: item.span.revisionId,
              unit: item.span.unit,
              start: item.span.start,
              end: item.span.end,
            },
    })),
  };
}

function authorRelation(
  binding: SourceBinding,
  authorId: string | null,
): ModelSourceDocument['speakerRelation'] {
  if (authorId === null) {
    if (binding.allowUnknownContext) return 'unknown';
  } else if (authorId.length) {
    if (authorId === binding.targetAuthorId) return 'target';
    if (binding.allowedContextAuthorIds.includes(authorId)) return 'context';
  }
  throw new Error('Автор не включён в выбранный контекст.');
}

function validatePeriod(period: SourceBinding['period'], createdAt: string | null): void {
  const from = period.from === null ? -Infinity : Date.parse(period.from);
  const to = period.to === null ? Infinity : Date.parse(period.to);
  if (Number.isNaN(from) || Number.isNaN(to) || from > to)
    throw new Error('Неверный выбранный период.');
  if (createdAt === null) {
    if (period.includeUndated) return;
  } else {
    const timestamp = Date.parse(createdAt);
    if (Number.isFinite(timestamp) && timestamp >= from && timestamp <= to) return;
  }
  throw new Error('Документ вне выбранного периода.');
}
