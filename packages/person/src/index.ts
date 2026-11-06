import type { IntelligenceResult, PersonaRule, PersonProfile } from '@proteus-int/contracts';
import { structured, text } from '@proteus-int/intelligence';
import type { IntelligenceProvider } from '@proteus-int/intelligence';
export interface PersonPort {
  solve(
    profile: PersonProfile,
    task: string,
    provider: IntelligenceProvider,
    signal?: AbortSignal,
  ): Promise<IntelligenceResult>;
}
export const personRuntime: PersonPort = {
  async solve(profile, task, provider, signal) {
    const { data, trace } = await structured(
      provider,
      'PERSON',
      'Solve the task using the supplied frozen archetype and skills as tentative behavioral guidance. Preserve the uncertainty and limitations in the rules; hypotheses are not established traits. For synthetic profiles do not claim to model an actual person accurately; for the neutral control do not invent personal traits. Do not invent differences for contrast. Give a concrete useful final answer, distinguish assumptions from known information and state relevant limitations. Return {"answer":string, "summary":string}; summary is a short observable outcome, never private reasoning. Do not claim to be the actual person.',
      {
        task,
        archetype: profile.archetype,
        skills: profile.skills,
        rules: profile.rules,
        'task-skills': profile['task-skills'],
        'source-status': profile['source-status'],
      },
      signal,
      'high',
      (data) => ({ answer: text(data.answer, 'answer'), summary: text(data.summary, 'summary') }),
    );
    return { answer: data.answer, trace: { ...trace, summary: data.summary } };
  },
};
export function personaArtifacts(profile: PersonProfile): Record<string, string> {
  const provenance = (rules: readonly PersonaRule[]) =>
    rules
      .map(
        (rule) =>
          `- ${rule.id}: ${rule.instruction}\n  Status: ${rule.status}; evidence: ${rule['evidence-ids'].join(', ') || 'none (task strategy)'}\n  Limitations: ${rule.limitations.join('; ') || 'No additional limitations reported; not independently verified.'}`,
      )
      .join('\n');
  const files: Record<string, string> = {
    'AGENTS.md': `# Simulated persona\n\nSource status: ${profile['source-status']}\n\n${profile.archetype}\n\n## Evidence and limitations\n\n${provenance(profile.rules) || 'Insufficient personal evidence; no characteristic traits asserted.'}\n\nUse the attached skills to approach the task. This is a behavioral simulation, not the real person. Source support is model-reported, not independent verification.\n`,
  };
  profile.skills.forEach((skill, index) => {
    files[`.agents/skills/persona-${index + 1}/SKILL.md`] =
      `---\nname: persona-${index + 1}\ndescription: Behavioral strategy for this persona's task\n---\n\n${skill}\n\n${provenance(profile['task-skills'].slice(index, index + 1))}\n`;
  });
  return files;
}
