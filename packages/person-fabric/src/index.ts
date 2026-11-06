import type {
  PersonInput,
  PersonProfile,
  IntelligenceTrace,
  PaletteId,
} from '@proteus-int/contracts';
import { structured, text, texts } from '@proteus-int/intelligence';
import type { IntelligenceProvider } from '@proteus-int/intelligence';
import { personRuntime } from '@proteus-int/person';
import type { PersonPort } from '@proteus-int/person';
import { anonymizeReport } from '@proteus-int/stalker';
import type { StalkerReport } from '@proteus-int/stalker';
export interface FabricatedPerson {
  readonly profile: PersonProfile;
  readonly runtime: PersonPort;
  readonly trace: IntelligenceTrace;
}
export async function fabricatePerson(
  report: StalkerReport,
  input: PersonInput,
  task: string,
  id: string,
  palette: PaletteId,
  provider: IntelligenceProvider,
  signal?: AbortSignal,
): Promise<FabricatedPerson> {
  const { data, trace } = await structured(
    provider,
    'PERSON_FABRIC',
    'Create a task-specific simulated person from anonymized behavior evidence. Return {"archetype":string, "skills":[string]}. Archetype is a detailed AGENTS.md-style behavioral instruction. Skills are actionable Markdown instructions reflecting how this person would approach THIS task, with priorities, characteristic decisions, limitations and tradeoffs. Make the approach recognizably human and individual, not generic role-play. Separate hypotheses from evidence, never infer sensitive traits. Do not introduce identities. Produce 2–5 concise skills.',
    { task, behavior: anonymizeReport(report, input) },
    signal,
    report.effort,
    (data) => {
      const skills = texts(data.skills, 'skills');
      if (skills.length < 1 || skills.length > 5) throw new Error('skills: expected 1–5 strings');
      return { archetype: text(data.archetype, 'archetype'), skills };
    },
  );
  return {
    profile: { id, label: input.label, palette, archetype: data.archetype, skills: data.skills },
    runtime: personRuntime,
    trace,
  };
}
