import type { Evidence, EvidenceStatus } from '@proteus-int/contracts';
import { decodeResponseFields, text, texts } from '@proteus-int/intelligence';
import type { StalkerReport } from './report';
import { validatePublicUrl } from './public-url';

export function parsePublicReport(raw: Record<string, unknown>): StalkerReport {
  const data = decodeResponseFields(raw, [
    ['thinking-style', 'thinkingStyle'],
    ['behavior-summary', 'behaviorSummary'],
  ]);
  if (!Array.isArray(data.evidence) || data.evidence.length > 30)
    throw new Error('STALKER: invalid evidence');
  const ids = new Set<string>();
  const evidence = data.evidence.map((value): Evidence => {
    const item = decodeResponseFields(value, [['source-url', 'sourceUrl']]);
    if (item.kind !== 'fact' && item.kind !== 'hypothesis')
      throw new Error('STALKER: invalid evidence kind');
    const source = typeof item.sourceUrl === 'string' ? item.sourceUrl : '';
    if (source) validatePublicUrl(source);
    if (item.kind === 'fact' && !source) throw new Error('STALKER: facts require a public source');
    const id = text(item.id, 'evidence.id');
    if (!/^e\d+$/u.test(id)) throw new Error('STALKER: evidence.id must be opaque, e.g. e1');
    if (ids.has(id)) throw new Error('STALKER: duplicate evidence.id');
    ids.add(id);
    if (
      typeof item.status !== 'string' ||
      !['supported', 'hypothesis', 'unknown', 'conflicting'].includes(item.status)
    )
      throw new Error('STALKER: invalid evidence.status');
    const excerpt = item.excerpt;
    if (
      excerpt !== null &&
      (typeof excerpt !== 'string' || !excerpt.trim() || excerpt.length > 24000)
    )
      throw new Error('STALKER: invalid evidence.excerpt');
    if (item.status === 'supported' && (!source || !excerpt || item.kind !== 'fact'))
      throw new Error('STALKER: supported evidence requires a fact, source and excerpt');
    return {
      id,
      sourceUrl: source,
      kind: item.kind,
      summary: text(item.summary, 'evidence.summary'),
      status: item.status as EvidenceStatus,
      verification: source && excerpt ? 'model-reported' : 'unverified',
      excerpt,
      // The adapter does not attest retrieval; never trust a model-supplied timestamp.
      retrievedAt: null,
      limitations: texts(item.limitations, 'evidence.limitations'),
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
