import type { EvidenceStatus, PersonProfile, IntelligenceTrace } from '@proteus-int/contracts';
import { structured } from '@proteus-int/intelligence';
import type { IntelligenceProvider } from '@proteus-int/intelligence';
import { parseRules, ruleShape } from './parse-rules';

export async function adaptPerson(
  profile: PersonProfile,
  task: string,
  provider: IntelligenceProvider,
  signal?: AbortSignal,
): Promise<{ profile: PersonProfile; trace: IntelligenceTrace }> {
  const evidence = new Map<string, EvidenceStatus>();
  for (const rule of profile.rules) {
    for (const id of rule.evidenceIds) {
      // Retain the more cautious interpretation if several rules reuse the same evidence.
      if (!evidence.has(id) || rule.status !== 'supported') evidence.set(id, rule.status);
    }
  }
  const { data: skills, trace } = await structured(
    provider,
    'PERSON_FABRIC_ADAPT',
    `Adapt the frozen behavioral rules to the task without changing the archetype, inventing personal capabilities or solving the task in advance. Return {skills:[${ruleShape.replace('r1', 's1')}]}. Produce 1–5 concise procedural strategies, not the answer. Each personalized strategy must cite supplied evidence IDs and preserve their hypothesis status and limitations. A general task strategy may have empty evidenceIds but must be hypothesis, never a newly discovered personal trait. For a neutral control, use general task strategies with empty evidenceIds only. Preserve supplied identity and relationship context; do not invent identities or sensitive traits. Do not manufacture differences between participants.`,
    {
      task,
      personaLabel: profile.sourceStatus === 'neutral' ? null : profile.label,
      archetype: profile.archetype,
      rules: profile.rules,
      sourceStatus: profile.sourceStatus,
    },
    signal,
    'medium',
    (data) => {
      const parsed = parseRules(data.skills, evidence, true);
      if (parsed.length < 1 || parsed.length > 5)
        throw new Error('skills: expected 1–5 strategies');
      return parsed;
    },
    'generation',
    profile.sourceContext === true,
  );
  const cleanSkills = skills.map((skill) => ({
    ...skill,
    limitations: [
      ...new Set([
        ...skill.limitations,
        ...profile.rules
          .filter((rule) => rule.evidenceIds.some((id) => skill.evidenceIds.includes(id)))
          .flatMap((rule) => rule.limitations),
      ]),
    ],
  }));
  return {
    profile: {
      ...profile,
      skills: cleanSkills.map((skill) => skill.instruction),
      taskSkills: cleanSkills,
    },
    trace,
  };
}
