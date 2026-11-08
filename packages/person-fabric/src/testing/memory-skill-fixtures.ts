import type { MemoryItem, SourceSpan } from '@proteus-int/contracts';
import type { MemorySkillScope } from '../parse-memory-skills';

export const encoder = new TextEncoder();

export interface SkillItemFixtureOptions {
  readonly id?: string;
  readonly snapshotId?: string;
  readonly kind?: MemoryItem['kind'];
  readonly support?: MemoryItem['support'];
  readonly text?: string;
  readonly documentId?: string;
  readonly revisionId?: string;
  readonly spanStart?: number;
  readonly contradictionIds?: readonly string[];
  readonly limitations?: readonly string[];
}

export function skillItemSpan(options: SkillItemFixtureOptions = {}): SourceSpan {
  const text = options.text ?? 'Сначала проверь границы, потом пиши код.';
  const start = options.spanStart ?? 0;
  return {
    documentId: options.documentId ?? 'doc-1',
    revisionId: options.revisionId ?? 'rev-1',
    unit: 'utf8Bytes',
    start,
    end: start + encoder.encode(text).length,
  };
}

export function skillMemoryItem(options: SkillItemFixtureOptions = {}): MemoryItem {
  return {
    schemaVersion: 1,
    id: options.id ?? 'item-1',
    snapshotId: options.snapshotId ?? 'snapshot-1',
    kind: options.kind ?? 'procedure',
    text: options.text ?? 'Сначала проверь границы, потом пиши код.',
    sourceSpans: [skillItemSpan(options)],
    episodeIds: ['episode-1'],
    eventAt: null,
    observedAt: '2026-06-01T00:00:00Z',
    support: options.support ?? 'observed',
    contradictionIds: options.contradictionIds ?? [],
    limitations: options.limitations ?? [],
    extractionVersion: 'memory-v1',
    policyVersion: 'policy-v1',
  };
}

export function skillMemoryScope(
  items: readonly MemoryItem[],
  options: { readonly maxSkills?: number } = {},
): MemorySkillScope {
  return { items, maxSkills: options.maxSkills ?? 10 };
}
