import type { PersonScore } from '@proteus-int/contracts';
export interface ScoringInput {
  readonly 'person-id': string;
  readonly 'reasoning-ms': number;
  readonly 'terminal-ms': number;
  readonly 'estimated-bsc': number;
}
export function scorePerson(input: ScoringInput): PersonScore {
  const estimate = input['estimated-bsc'];
  if (!Number.isFinite(estimate) || estimate <= 0)
    throw new RangeError('Estimated BSC must be positive and finite');
  for (const duration of [input['reasoning-ms'], input['terminal-ms']]) {
    if (!Number.isFinite(duration) || duration < 0)
      throw new RangeError('Durations must be non-negative and finite');
  }
  const minutes = input['reasoning-ms'] / 60_000;
  const bsc = minutes / estimate;
  if (!Number.isFinite(bsc)) throw new RangeError('BSC exceeds the supported numeric range');
  return { 'person-id': input['person-id'], 'reasoning-minutes': minutes, bsc };
}
