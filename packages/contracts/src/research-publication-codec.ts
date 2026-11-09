import type {
  MemorySnapshot,
  ResearchPublicationDraft,
  ResearchPublicationReceipt,
} from './memory';
import type { ResearchSessionDescriptor } from './research-memory';
import type { CorpusCoverage, RevisionReference, SourceSpan } from './corpus';
import { decodeCorpusCoverage } from './corpus-codec';
import { fields, list, version } from './corpus-wire-validation';
import { decodeSourceSpan } from './source-excerpt-codec';
import { memoryId, memoryIds, memoryText, memoryTimestamp } from './memory-wire-validation';

const invalid = 'Публикация не соответствует исследованию.';
function referenceKey(value: RevisionReference): string {
  return JSON.stringify([value.documentId, value.revisionId]);
}
function snapshot(
  value: unknown,
  session: ResearchSessionDescriptor,
  draft: ResearchPublicationDraft,
): MemorySnapshot {
  const input = fields(value, [
    'schemaVersion',
    'id',
    'corpusSnapshotId',
    'itemIds',
    'extractionVersion',
    'createdAt',
  ]);
  version(input.schemaVersion);
  if (
    input.corpusSnapshotId !== session.corpusSnapshotId ||
    input.extractionVersion !== draft.extractionVersion
  )
    throw new Error(invalid);
  return {
    schemaVersion: 1,
    id: memoryId(input.id),
    corpusSnapshotId: session.corpusSnapshotId,
    itemIds: memoryIds(input.itemIds),
    extractionVersion: memoryId(input.extractionVersion),
    createdAt: memoryTimestamp(input.createdAt),
  };
}
function analyzed(value: unknown, draft: ResearchPublicationDraft): SourceSpan[] {
  const values = list(value);
  if (values.length > 100000 || draft.analyzedRefs.length > 10000) throw new Error(invalid);
  const spans = values.map((value) => {
    const span = decodeSourceSpan(value);
    memoryId(span.documentId);
    memoryId(span.revisionId);
    return span;
  });
  const unique = new Set(
    spans.map((span) => JSON.stringify([span.documentId, span.revisionId, span.start, span.end])),
  );
  const refs = new Set(spans.map(referenceKey));
  const expected = new Set(draft.analyzedRefs.map(referenceKey));
  if (
    unique.size !== spans.length ||
    expected.size !== draft.analyzedRefs.length ||
    refs.size !== expected.size ||
    [...refs].some((ref) => !expected.has(ref))
  )
    throw new Error(invalid);
  return spans;
}

// Sweep by start within each revision. The maximum end belongs to one actual
// interval: adjacent intervals are never joined into invented source support.
function validateSupport(analyzedSpans: readonly SourceSpan[], used: readonly SourceSpan[]): void {
  const byRevision = new Map<string, SourceSpan[]>();
  for (const span of analyzedSpans) {
    const key = referenceKey(span);
    const existing = byRevision.get(key);
    if (existing) existing.push(span);
    else byRevision.set(key, [span]);
  }
  const requested = new Map<string, SourceSpan[]>();
  for (const span of used) {
    const key = referenceKey(span);
    const existing = requested.get(key);
    if (existing) existing.push(span);
    else requested.set(key, [span]);
  }
  for (const [key, spans] of requested) {
    const candidates = byRevision.get(key) ?? [];
    candidates.sort((a, b) => a.start - b.start);
    spans.sort((a, b) => a.start - b.start);
    let next = 0;
    let end = -1;
    for (const span of spans) {
      while (next < candidates.length && candidates[next]!.start <= span.start) {
        end = Math.max(end, candidates[next]!.end);
        next++;
      }
      if (end < span.end) throw new Error(invalid);
    }
  }
}
function coverage(
  value: unknown,
  spans: readonly SourceSpan[],
  used: readonly SourceSpan[],
): CorpusCoverage {
  const result = decodeCorpusCoverage(value);
  if (
    result.analyzedDocuments > result.indexedDocuments ||
    result.usedDocuments > result.analyzedDocuments ||
    result.analyzedDocuments !== new Set(spans.map((span) => span.documentId)).size ||
    result.usedDocuments !== new Set(used.map((span) => span.documentId)).size ||
    result.limitations.length > 16
  )
    throw new Error(invalid);
  for (const limitation of result.limitations)
    if (!memoryText(limitation, 2048).trim()) throw new Error(invalid);
  return result;
}

/** Structural receipt binding only. Native owns durable publication and completed-call evidence. */
export function decodeResearchPublication(
  value: unknown,
  session: ResearchSessionDescriptor,
  draft: ResearchPublicationDraft,
): ResearchPublicationReceipt {
  const input = fields(value, [
    'schemaVersion',
    'id',
    'snapshot',
    'itemIds',
    'coverage',
    'analyzedSpans',
    'publishedAt',
  ]);
  version(input.schemaVersion);
  const stored = snapshot(input.snapshot, session, draft);
  const mappings = list(input.itemIds);
  if (
    mappings.length !== draft.items.length ||
    mappings.length !== stored.itemIds.length ||
    mappings.length > 1000
  )
    throw new Error(invalid);
  const itemIds = mappings.map((value, index) => {
    const item = fields(value, ['draftId', 'itemId']);
    if (item.draftId !== draft.items[index]?.draftId || item.itemId !== stored.itemIds[index])
      throw new Error(invalid);
    return { draftId: memoryId(item.draftId), itemId: memoryId(item.itemId) };
  });
  if (new Set(itemIds.map((item) => item.draftId)).size !== itemIds.length)
    throw new Error(invalid);
  const publishedAt = memoryTimestamp(input.publishedAt);
  if (publishedAt !== stored.createdAt) throw new Error(invalid);
  const analyzedSpans = analyzed(input.analyzedSpans, draft);
  const used = draft.items.flatMap((item) => item.sourceSpans);
  validateSupport(analyzedSpans, used);
  return {
    schemaVersion: 1,
    id: memoryId(input.id),
    snapshot: stored,
    itemIds,
    coverage: coverage(input.coverage, analyzedSpans, used),
    analyzedSpans,
    publishedAt,
  };
}
