import type { IntelligenceResult, PersonScore, PersonaObservation } from '@proteus-int/contracts';
import { scorePerson } from '@proteus-int/brain-scoring';
import type { ScoringInput } from '@proteus-int/brain-scoring';
import { object, structured, text, texts } from '@proteus-int/intelligence';
import type { IntelligenceProvider } from '@proteus-int/intelligence';
export interface BrainAnalysis {
  readonly summary: string;
  readonly scores: readonly PersonScore[];
  readonly 'key-differences': readonly string[];
  readonly people: readonly PersonaObservation[];
  readonly ratio: number | null;
}
export async function analyzePeople(
  task: string,
  attempts: readonly IntelligenceResult[],
  scoring: readonly ScoringInput[],
  provider: IntelligenceProvider,
  signal?: AbortSignal,
): Promise<BrainAnalysis> {
  const scores = scoring.map(scorePerson);
  const { data } = await structured(
    provider,
    'BRAIN_ANALYZATOR',
    'Compare the two simulated task attempts in their input order. Return {summary, "key-differences":[string], people:[{strengths:[string], weaknesses:[string], observations:[string]}, {strengths:[string], weaknesses:[string], observations:[string]}]}. Assess strengths, weaknesses, interesting behavioral differences and concrete answer quality. BSC is reasoning/answer wall time in minutes divided by a speculative estimate; terminal time is excluded. Do not describe its ratio as human intelligence, IQ or scientific superiority. Timing includes provider latency and is only an operational proxy.',
    { task, attempts, scores },
    signal,
    'high',
  );
  if (!Array.isArray(data.people) || data.people.length !== 2)
    throw new Error('BRAIN_ANALYZATOR: expected two persona analyses');
  const people = data.people.map((value): PersonaObservation => {
    const item = object(value);
    return {
      strengths: texts(item.strengths),
      weaknesses: texts(item.weaknesses),
      observations: texts(item.observations),
    };
  });
  const a = scores[0]?.bsc ?? 0;
  const b = scores[1]?.bsc ?? 0;
  const ratio = a > 0 && b > 0 ? Math.max(a, b) / Math.min(a, b) : null;
  return {
    summary: text(data.summary),
    scores,
    'key-differences': texts(data['key-differences']),
    people,
    ratio: ratio !== null && Number.isFinite(ratio) ? ratio : null,
  };
}
