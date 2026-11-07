import type { EvaluationRubric, IntelligenceTrace } from '@proteus-int/contracts';
import { structured } from '@proteus-int/intelligence';
import type { IntelligenceProvider } from '@proteus-int/intelligence';
import { parseRubric } from './parse-rubric';
export async function createRubric(
  task: string,
  provider: IntelligenceProvider,
  signal?: AbortSignal,
): Promise<{ rubric: EvaluationRubric; trace: IntelligenceTrace }> {
  const { data, trace } = await structured(
    provider,
    'BRAIN_ANALYZATOR_RUBRIC',
    'Create a task-specific assessment rubric before any answers or persona profiles exist. Return {criteria:[{id,label,weight,required,anchors:{unmet,partial,met}}]}. Use 1–12 distinct kebab-case IDs, positive finite numeric weights, boolean required flags, and observable anchored definitions for 0 (unmet), 0.5 (partial) and 1 (met). Required means a critical task constraint, not a generic preference. Assess task completion, correctness and constraints, not personality, verbosity, speed, or hidden reasoning. Do not invent external facts or reference answers. Criteria must apply equally to every participant. If correctness needs unavailable verification, explicitly allow unknown rather than rewarding confident wording. Do not introduce requirements absent from the task.',
    { task },
    signal,
    'high',
    parseRubric,
  );
  return { rubric: data, trace };
}
