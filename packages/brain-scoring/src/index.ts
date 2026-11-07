import type { PersonScore } from '@proteus-int/contracts';
export { difference, scoreTrial, summarizeScores, validateRubric } from './evaluation';
export interface ScoringInput {
  readonly personId: string;
  readonly reasoningMs: number;
  readonly terminalMs: number;
  readonly estimatedBsc: number;
}
export function scorePerson(input: ScoringInput): PersonScore {
  const estimate = input.estimatedBsc;
  if (!Number.isFinite(estimate) || estimate <= 0)
    throw new RangeError('Estimated BSC must be positive and finite');
  for (const duration of [input.reasoningMs, input.terminalMs]) {
    if (!Number.isFinite(duration) || duration < 0)
      throw new RangeError('Durations must be non-negative and finite');
  }
  const minutes = input.reasoningMs / 60_000;
  const bsc = minutes / estimate;
  if (!Number.isFinite(bsc)) throw new RangeError('BSC exceeds the supported numeric range');
  return { personId: input.personId, reasoningMinutes: minutes, bsc };
}
