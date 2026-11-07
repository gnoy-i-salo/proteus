import type { Evidence, EvidenceStatus, SourceDocument } from '@proteus-int/contracts';
import { decodeResponseFields, text, texts } from '@proteus-int/intelligence';
import type { StalkerReport } from './report';
import type { sourceModelDocument } from './source-model-document';

export function parseAnalysis(
  rawResponse: Record<string, unknown>,
  docs: readonly SourceDocument[],
  modelDocuments: ReturnType<typeof sourceModelDocument>[],
): StalkerReport {
  const data = decodeResponseFields(rawResponse, [
    ['thinking-style', 'thinkingStyle'],
    ['behavior-summary', 'behaviorSummary'],
  ]);
  if (!Array.isArray(data.evidence) || data.evidence.length > 6)
    throw new Error('STALKER: invalid corpus evidence');
  const evidence = data.evidence.map((raw): Evidence => {
    const item = decodeResponseFields(raw, [['document-ids', 'documentIds']]);
    const ids = texts(item.documentIds, 'documentIds');
    if (
      ids.length > 5 ||
      new Set(ids).size !== ids.length ||
      ids.some((id) => !docs.some((doc) => doc.id === id))
    )
      throw new Error('STALKER: unknown or duplicate document reference');
    if (
      (item.kind !== 'fact' && item.kind !== 'hypothesis') ||
      typeof item.status !== 'string' ||
      !['supported', 'hypothesis', 'unknown', 'conflicting'].includes(item.status)
    )
      throw new Error('STALKER: invalid corpus evidence status or kind');
    if (item.kind === 'fact' && ids.length === 0)
      throw new Error('STALKER: fact requires document reference');
    const excerpt = item.excerpt;
    if (
      excerpt !== null &&
      (typeof excerpt !== 'string' ||
        !excerpt.length ||
        excerpt.length > 1200 ||
        !modelDocuments.some((doc) => ids.includes(doc.id) && doc.text.includes(excerpt)))
    )
      throw new Error('STALKER: excerpt must be an exact source substring');
    const cited = docs.filter((doc) => ids.includes(doc.id));
    if (
      item.status === 'supported' &&
      (item.kind !== 'fact' ||
        !excerpt ||
        !cited.length ||
        cited.some((doc) => doc.authorship !== 'owner' || doc.isForwarded))
    )
      throw new Error(
        'STALKER: supported observations need authored document evidence and excerpt',
      );
    const summary = text(item.summary, 'evidence.summary');
    if (summary.length > 1000) throw new Error('STALKER: evidence summary exceeds limit');
    const source = cited[0];
    return {
      id: 'pending',
      kind: item.kind,
      status: item.status as EvidenceStatus,
      summary,
      documentIds: ids,
      excerpt: excerpt as string | null,
      sourceUrl: source?.sourceUrl ?? '',
      retrievedAt: source?.retrievedAt ?? null,
      verification: cited.length ? 'source-retrieved' : 'unverified',
      limitations: [
        ...texts(item.limitations, 'evidence.limitations'),
        ...cited.filter((doc) => doc.truncated).map(() => 'Source text is truncated'),
        'Source retrieval does not independently verify the assertion',
      ],
    };
  });
  return {
    evidence,
    character: text(data.character, 'character'),
    thinkingStyle: text(data.thinkingStyle, 'thinkingStyle'),
    activity: text(data.activity, 'activity'),
    behaviorSummary: text(data.behaviorSummary, 'behaviorSummary'),
  };
}
