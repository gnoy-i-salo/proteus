import type { IntelligenceResult, PersonProfile } from '@proteus-int/contracts';
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
      'Solve the task using the supplied archetype and skills as behavioral guidance. Act with that simulated persona’s priorities, habits, tradeoffs and limitations; do not claim to be the actual person. Give a concrete useful final answer. Return {"answer":string, "summary":string}; summary is a short observable outcome, never private reasoning.',
      { task, archetype: profile.archetype, skills: profile.skills },
      signal,
      'high',
      (data) => ({ answer: text(data.answer, 'answer'), summary: text(data.summary, 'summary') }),
    );
    return { answer: data.answer, trace: { ...trace, summary: data.summary } };
  },
};
export function personaArtifacts(profile: PersonProfile): Record<string, string> {
  const files: Record<string, string> = {
    'AGENTS.md': `# Simulated persona\n\n${profile.archetype}\n\nUse the attached skills to approach the task. This is a behavioral simulation, not the real person.\n`,
  };
  profile.skills.forEach((skill, index) => {
    files[`.agents/skills/persona-${index + 1}/SKILL.md`] =
      `---\nname: persona-${index + 1}\ndescription: Behavioral strategy for this persona's task\n---\n\n${skill}\n`;
  });
  return files;
}
