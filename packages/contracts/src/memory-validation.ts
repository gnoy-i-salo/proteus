import type {
  RecallBudget,
  RetrievalReceipt,
  EmbeddingFingerprint,
  ModelMemoryAction,
} from './memory';
import type { RevisionReference } from './corpus';
import { decodeRetrievalReceipt, decodeEmbeddingFingerprint } from './memory-retrieval-codec';
import { memoryFields as fields, memoryReceiptText as text } from './memory-wire-validation';

export interface RetrievalReceiptScope {
  readonly operation: 'recall' | 'readEpisode';
  readonly snapshotId: string;
  readonly policyVersion: string;
  readonly retrievalVersion: string;
  readonly indexGeneration: number;
  readonly sourceRefs: readonly RevisionReference[];
  readonly embeddingFingerprint: EmbeddingFingerprint | null;
  readonly budget: RecallBudget;
}

/** A structural receipt still needs the expected bound source scope. Native owns live authority. */
export function validateRetrievalReceipt(
  value: unknown,
  scope: RetrievalReceiptScope,
): RetrievalReceipt {
  const receipt = decodeRetrievalReceipt(value, scope.budget);
  if (
    receipt.operation !== scope.operation ||
    receipt.snapshotId !== scope.snapshotId ||
    receipt.policyVersion !== scope.policyVersion ||
    receipt.retrievalVersion !== scope.retrievalVersion ||
    receipt.indexGeneration !== scope.indexGeneration
  )
    throw new Error('Memory receipt version or bound scope mismatch.');
  if (
    JSON.stringify(receipt.embeddingFingerprint) !==
    JSON.stringify(decodeEmbeddingFingerprint(scope.embeddingFingerprint))
  )
    throw new Error('Memory receipt embedding space mismatch.');
  const allowed = new Set(
    scope.sourceRefs.map((ref) => JSON.stringify([ref.documentId, ref.revisionId])),
  );
  if (
    receipt.hits.some((hit) =>
      hit.spans.some((span) => !allowed.has(JSON.stringify([span.documentId, span.revisionId]))),
    )
  )
    throw new Error('Memory receipt span is outside the bound revisions.');
  return receipt;
}

/** Model action schema only; native still authorizes every call in its bound session. */
export function parseMemoryAction(value: unknown): ModelMemoryAction {
  const action = value && typeof value === 'object' && 'action' in value ? value.action : undefined;
  if (action === 'recall') {
    const input = fields(value, ['action', 'query', 'intent']);
    if (
      typeof input.query !== 'string' ||
      !input.query.trim() ||
      new TextEncoder().encode(input.query).length > 4096
    )
      throw new Error('Memory recall query exceeds the nonempty 4096-byte tool limit.');
    if (
      input.intent !== 'facts' &&
      input.intent !== 'context' &&
      input.intent !== 'style' &&
      input.intent !== 'procedure'
    )
      throw new Error('Invalid memory recall intent.');
    return { action: 'recall', query: input.query, intent: input.intent };
  }
  if (action === 'readEpisode') {
    const input = fields(value, ['action', 'episodeId']);
    const episodeId = text(input.episodeId);
    if (episodeId.length > 256) throw new Error('Invalid memory episode ID.');
    return { action: 'readEpisode', episodeId };
  }
  throw new Error('Only recall and readEpisode are model memory tools.');
}
