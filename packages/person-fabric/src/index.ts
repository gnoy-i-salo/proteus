import type {
  EvidenceStatus,
  PersonInput,
  PersonProfile,
  PersonaRule,
  IntelligenceTrace,
  PaletteId,
} from '@proteus-int/contracts';
import { object, structured, text, texts } from '@proteus-int/intelligence';
import type { IntelligenceProvider } from '@proteus-int/intelligence';
import { personRuntime } from '@proteus-int/person';
import type { PersonPort } from '@proteus-int/person';
import { anonymizeReport, redactTask } from '@proteus-int/stalker';
import type { StalkerReport } from '@proteus-int/stalker';

export interface FabricatedPerson {
  readonly profile: PersonProfile;
  readonly runtime: PersonPort;
  readonly trace: IntelligenceTrace;
}
function parseRules(
  raw: unknown,
  evidence: ReadonlyMap<string, EvidenceStatus>,
  taskSkills = false,
): PersonaRule[] {
  if (!Array.isArray(raw) || raw.length > 10) throw new Error('rules: expected at most 10 rules');
  const ids = new Set<string>();
  return raw.map((value) => {
    const rule = object(value);
    const id = text(rule.id, 'rule.id');
    if (!/^[rs]\d+$/u.test(id) || ids.has(id))
      throw new Error('rule.id: expected unique opaque ID');
    ids.add(id);
    const references = texts(rule['evidence-ids'], 'rule.evidence-ids');
    if (new Set(references).size !== references.length)
      throw new Error('rule.evidence-ids: duplicate reference');
    if (!references.length && !taskSkills)
      throw new Error('rule.evidence-ids: profile rules need evidence');
    if (references.some((reference) => !evidence.has(reference)))
      throw new Error('rule.evidence-ids: unknown evidence reference');
    if (rule.status !== 'supported' && rule.status !== 'hypothesis')
      throw new Error('rule.status: unknown or conflicting evidence cannot drive behavior');
    if (
      references.some((reference) => ['unknown', 'conflicting'].includes(evidence.get(reference)!))
    )
      throw new Error('rule.evidence-ids: unknown or conflicting evidence cannot drive behavior');
    if (
      rule.status === 'supported' &&
      (!references.length ||
        references.some((reference) => evidence.get(reference) !== 'supported'))
    )
      throw new Error('rule.status: supported rules require supported evidence');
    return {
      id,
      instruction: text(rule.instruction, 'rule.instruction'),
      'evidence-ids': references,
      status: rule.status,
      limitations: texts(rule.limitations, 'rule.limitations'),
    };
  });
}
const ruleShape =
  '{id:"r1", instruction:string, "evidence-ids":["e1"], status:"supported"|"hypothesis", limitations:[string]}';

export async function fabricatePerson(
  report: StalkerReport,
  input: PersonInput,
  id: string,
  palette: PaletteId,
  provider: IntelligenceProvider,
  signal?: AbortSignal,
): Promise<FabricatedPerson> {
  const behavior = anonymizeReport(report, input);
  const { data: rules, trace } = await structured(
    provider,
    'PERSON_FABRIC',
    `Create a stable simulated behavioral profile using only the supplied evidence, without anticipating any task. Return {rules:[${ruleShape}]}. Each rule must link to existing opaque evidence IDs. Preserve hypothesis status and limitations: supported means source support reported by a model, not independent verification. Never upgrade a hypothesis. Unknown or conflicting evidence cannot justify a behavioral instruction; omit such rules. Empty rules is valid when information is insufficient. Do not invent sensitive traits, identities, abilities or differences for contrast. Identical profiles are allowed. Do not treat unlinked summary fields as evidence. Rules are tentative behavioral guidance, not claims about a real person's mind.`,
    { evidence: behavior.evidence },
    signal,
    'medium',
    (data) =>
      parseRules(data.rules, new Map(behavior.evidence.map((item) => [item.id, item.status]))),
  );
  const cleanRules = rules.map((rule) => ({
    ...rule,
    instruction: redactTask(rule.instruction, [input]),
    limitations: [
      ...new Set([
        ...rule.limitations,
        ...behavior.evidence
          .filter((item) => rule['evidence-ids'].includes(item.id))
          .flatMap((item) => item.limitations),
      ]),
    ].map((limitation) => redactTask(limitation, [input])),
  }));
  const sourceStatus = cleanRules.some((rule) => rule.status === 'supported')
    ? 'evidence-informed'
    : 'synthetic';
  const archetype = cleanRules.length
    ? cleanRules
        .map((rule) => `[${rule.status}; ${rule['evidence-ids'].join(', ')}] ${rule.instruction}`)
        .join('\n')
    : 'Недостаточно сведений для характерных черт. Не приписывайте человеку особенности или способности.';
  return {
    profile: {
      id,
      label: input.label,
      palette,
      archetype,
      skills: [],
      rules: cleanRules,
      'task-skills': [],
      'source-status': sourceStatus,
    },
    runtime: personRuntime,
    trace,
  };
}

export function neutralProfile(): PersonProfile {
  return {
    id: 'neutral',
    label: 'Нейтральный контроль',
    palette: 'turquoise',
    archetype: 'Решайте общую задачу без приписанных персональных черт, привычек или способностей.',
    skills: [],
    rules: [],
    'task-skills': [],
    'source-status': 'neutral',
  };
}

export async function adaptPerson(
  profile: PersonProfile,
  task: string,
  provider: IntelligenceProvider,
  signal?: AbortSignal,
): Promise<{ profile: PersonProfile; trace: IntelligenceTrace }> {
  const evidence = new Map<string, EvidenceStatus>();
  for (const rule of profile.rules) {
    for (const id of rule['evidence-ids']) {
      // Retain the more cautious interpretation if several rules reuse the same evidence.
      if (!evidence.has(id) || rule.status !== 'supported') evidence.set(id, rule.status);
    }
  }
  const { data: skills, trace } = await structured(
    provider,
    'PERSON_FABRIC_ADAPT',
    `Adapt the frozen behavioral rules to the task without changing the archetype, inventing personal capabilities or solving the task in advance. Return {skills:[${ruleShape.replace('r1', 's1')}]}. Produce 1–5 concise procedural strategies, not the answer. Each personalized strategy must cite supplied evidence IDs and preserve their hypothesis status and limitations. A general task strategy may have empty evidence-ids but must be hypothesis, never a newly discovered personal trait. For a neutral control, use general task strategies with empty evidence-ids only. No sensitive inferences or identities. Do not manufacture differences between participants.`,
    {
      task,
      archetype: profile.archetype,
      rules: profile.rules,
      'source-status': profile['source-status'],
    },
    signal,
    'medium',
    (data) => {
      const parsed = parseRules(data.skills, evidence, true);
      if (parsed.length < 1 || parsed.length > 5)
        throw new Error('skills: expected 1–5 strategies');
      return parsed;
    },
  );
  const cleanSkills = skills.map((skill) => ({
    ...skill,
    instruction: redactTask(skill.instruction, [
      { label: profile.label, context: '', 'public-urls': [] },
    ]),
    limitations: [
      ...new Set([
        ...skill.limitations,
        ...profile.rules
          .filter((rule) => rule['evidence-ids'].some((id) => skill['evidence-ids'].includes(id)))
          .flatMap((rule) => rule.limitations),
      ]),
    ].map((limitation) =>
      redactTask(limitation, [{ label: profile.label, context: '', 'public-urls': [] }]),
    ),
  }));
  return {
    profile: {
      ...profile,
      skills: cleanSkills.map((skill) => skill.instruction),
      'task-skills': cleanSkills,
    },
    trace,
  };
}
