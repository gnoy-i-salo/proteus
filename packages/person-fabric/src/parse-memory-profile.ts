import type {
  EvidenceStatus,
  FabricContextLease,
  MemoryItem,
  SourceExcerpt,
  StyleExemplar,
} from '@proteus-int/contracts';
import { parseRules } from './parse-rules';
import { parseStyle } from './parse-style';
import { parseMemorySkills } from './parse-memory-skills';

function status(item: MemoryItem): EvidenceStatus {
  if (item.contradictionIds.length || item.support === 'conflicting') return 'conflicting';
  if (item.support === 'observed') return 'supported';
  if (item.support === 'reported' || item.support === 'hypothesis') return 'hypothesis';
  return 'unknown';
}
function exactSample(item: MemoryItem, source: SourceExcerpt): boolean {
  if (!source.canSupportOwnWords) return false;
  const bytes = new TextEncoder().encode(source.text);
  const decoder = new TextDecoder('utf-8', { fatal: true, ignoreBOM: true });
  return item.sourceSpans.some(
    (span) =>
      span.documentId === source.span.documentId &&
      span.revisionId === source.span.revisionId &&
      span.start >= source.span.start &&
      span.end <= source.span.end &&
      decoder.decode(
        bytes.subarray(span.start - source.span.start, span.end - source.span.start),
      ) === item.text,
  );
}
function samples(context: FabricContextLease): StyleExemplar[] {
  const selected = context.items.filter(
    (item) =>
      item.kind === 'styleExample' && item.support === 'observed' && !item.contradictionIds.length,
  );
  if (selected.length > 100) throw new Error('Слишком много образцов профиля.');
  return selected.map((item) => {
    const original = context.sources.find((source) => exactSample(item, source));
    if (!original) throw new Error('Нет точного собственного образца стиля.');
    return {
      id: item.id,
      documentId: original.span.documentId,
      mode: original.document.mode,
      text: item.text,
      context: JSON.stringify({
        sourceSpans: item.sourceSpans,
        document: original.document,
        relations: original.relations,
        ...(original.sourceMetadata ? { sourceMetadata: original.sourceMetadata } : {}),
        limitations: item.limitations,
      }),
      redactions: [],
      // A processing lease conveys no export permission. Export needs a separate
      // native check; never infer permission from successful model preparation.
      exportAllowed: false,
    };
  });
}
export function parseMemoryProfile(
  value: Record<string, unknown>,
  context: FabricContextLease,
  maxSkills: number,
) {
  if (
    Object.keys(value).length !== 3 ||
    ['rules', 'style', 'skills'].some((key) => !Object.hasOwn(value, key))
  )
    throw new Error('Fabric: нужны только rules, style и skills.');
  const byId = new Map(context.items.map((item) => [item.id, item]));
  const rules = parseRules(
    value.rules,
    new Map(context.items.map((item) => [item.id, status(item)])),
  ).map((rule) => ({
    ...rule,
    limitations: [
      ...new Set([
        ...rule.limitations,
        ...rule.evidenceIds.flatMap((id) => byId.get(id)!.limitations),
        ...(rule.evidenceIds.some((id) => byId.get(id)!.support === 'reported')
          ? ['Это сообщение источника, а не независимо подтверждённый факт.']
          : []),
      ]),
    ],
  }));
  const exemplars = samples(context);
  const style = parseStyle(value.style, exemplars);
  const skills = parseMemorySkills(
    { skills: value.skills },
    { items: context.items, maxSkills },
  ).map((skill) => ({
    ...skill,
    limitations: [
      ...new Set([
        ...skill.limitations,
        ...skill.memoryItemIds.flatMap((id) => byId.get(id)!.limitations),
      ]),
    ],
  }));
  return {
    rules,
    skills,
    exemplars,
    style: {
      ...style,
      limitations: [
        ...new Set([
          ...style.limitations,
          ...exemplars.flatMap((sample) => byId.get(sample.id)!.limitations),
        ]),
      ],
    },
  };
}
