import type { EvidenceStatus, PersonaRule } from '@proteus-int/contracts';
import { decodeResponseFields, text, texts } from '@proteus-int/intelligence';

export function parseRules(
  raw: unknown,
  evidence: ReadonlyMap<string, EvidenceStatus>,
  taskSkills = false,
): PersonaRule[] {
  if (!Array.isArray(raw) || raw.length > 10) throw new Error('rules: expected at most 10 rules');
  const ids = new Set<string>();
  return raw.map((value) => {
    const rule = decodeResponseFields(value, [['evidence-ids', 'evidenceIds']]);
    const id = text(rule.id, 'rule.id');
    if (!/^[rs]\d+$/u.test(id) || ids.has(id))
      throw new Error('rule.id: expected unique opaque ID');
    ids.add(id);
    const references = texts(rule.evidenceIds, 'rule.evidenceIds');
    if (new Set(references).size !== references.length)
      throw new Error('rule.evidenceIds: duplicate reference');
    if (!references.length && !taskSkills)
      throw new Error('rule.evidenceIds: profile rules need evidence');
    if (references.some((reference) => !evidence.has(reference)))
      throw new Error('rule.evidenceIds: unknown evidence reference');
    if (rule.status !== 'supported' && rule.status !== 'hypothesis')
      throw new Error('rule.status: unknown or conflicting evidence cannot drive behavior');
    if (
      references.some((reference) => ['unknown', 'conflicting'].includes(evidence.get(reference)!))
    )
      throw new Error('rule.evidenceIds: unknown or conflicting evidence cannot drive behavior');
    if (
      rule.status === 'supported' &&
      (!references.length ||
        references.some((reference) => evidence.get(reference) !== 'supported'))
    )
      throw new Error('rule.status: supported rules require supported evidence');
    return {
      id,
      instruction: text(rule.instruction, 'rule.instruction'),
      evidenceIds: references,
      status: rule.status,
      limitations: texts(rule.limitations, 'rule.limitations'),
    };
  });
}
export const ruleShape =
  '{id:"r1", instruction:string, evidenceIds:["e1"], status:"supported"|"hypothesis", limitations:[string]}';
