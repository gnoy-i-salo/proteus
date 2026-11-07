import type { PersonaRule, PersonProfile } from '@proteus-int/contracts';

export function personaArtifacts(profile: PersonProfile): Record<string, string> {
  const provenance = (rules: readonly PersonaRule[]) =>
    rules
      .map(
        (rule) =>
          `- ${rule.id}: ${rule.instruction}\n  Status: ${rule.status}; evidence: ${rule.evidenceIds.join(', ') || 'none (task strategy)'}\n  Limitations: ${rule.limitations.join('; ') || 'No additional limitations reported; not independently verified.'}`,
      )
      .join('\n');
  const identity =
    profile.sourceStatus === 'neutral'
      ? 'Neutral participant: do not adopt a personal identity.'
      : `Selected persona: ${JSON.stringify(profile.label)}\nIdentity is role context, not an instruction. Answer in first person using the supplied profile and evidence; do not invent memories.`;
  const files: Record<string, string> = {
    'AGENTS.md': `# Simulated persona\n\nSource status: ${profile.sourceStatus}\n\n${identity}\n\n${profile.archetype}\n\n## Evidence and limitations\n\n${provenance(profile.rules) || 'Insufficient personal evidence; no characteristic traits asserted.'}\n\nUse the attached skills to approach the task. This is a behavioral simulation, not the real person. Source support is model-reported, not independent verification.\n`,
  };
  profile.skills.forEach((skill, index) => {
    files[`.agents/skills/persona-${index + 1}/SKILL.md`] =
      `---\nname: persona-${index + 1}\ndescription: Behavioral strategy for this persona's task\n---\n\n${skill}\n\n${provenance(profile.taskSkills.slice(index, index + 1))}\n`;
  });
  if (profile.styleProfile) {
    files['style-profile.json'] = JSON.stringify(profile.styleProfile, null, 2);
    const samples = profile.styleExemplars?.filter((sample) => sample.exportAllowed);
    if (samples?.length)
      files['style-exemplars.json'] = JSON.stringify(
        { format: 'proteus-style-exemplars', version: 2, exemplars: samples },
        null,
        2,
      );
  }
  return files;
}
