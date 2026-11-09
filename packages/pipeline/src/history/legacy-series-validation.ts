// Decode primitives, size/text bounds and cross-reference integrity checks for
// the exact legacy series-result schemas. Bounds follow the pinned writers:
// validateExperiment (task/context 4000, label 80, five URLs), STALKER
// (30 evidence items, opaque eN ids), fabric (10 rules, 1–5 strategies),
// model settings (aliases 120 characters), person replies (1–30 messages,
// 200000 joined characters) and the desktop save envelope (8 MiB UTF-8).
import type {
  LegacySeriesExperimentPerson,
  LegacySeriesResult,
  LegacySeriesScoreInterval,
  LegacySeriesTrial,
} from './legacy-series-types';

export const maxResultBytes = 8 * 1024 * 1024;
export const taskMax = 4000;
export const labelMax = 80;
export const contextMax = 4000;
export const urlListMax = 5;
export const urlTextMax = maxResultBytes;
export const modelTextMax = 24000;
export const listMax = 30;
export const rulesMax = 10;
export const reasoningMax = maxResultBytes;
export const attemptsMax = maxResultBytes;
export const idMax = 120;
export const artifactCountMax = 256;
export const artifactKeyMax = 512;
export const artifactValueMax = 1024 * 1024;
export const replyMessagesMax = 30;
export const replyTotalMax = 200000;
// messages.join('') <= 200000 from the writer, joined with two LF characters
// between at most 30 messages, so a recorded answer never exceeds this.
export const answerTextMax = replyTotalMax + 2 * (replyMessagesMax - 1);

export function invalid(detail: string): never {
  throw new Error(`Invalid legacy series result: ${detail}`);
}

export function recordFields(
  value: unknown,
  requiredKeys: readonly string[],
  optionalKeys: readonly string[],
  context: string,
): Record<string, unknown> {
  if (!value || typeof value !== 'object' || Array.isArray(value))
    invalid(`${context} must be an object`);
  const source = value as Record<string, unknown>;
  const allowed = new Set<string>([...requiredKeys, ...optionalKeys]);
  const actual = Object.keys(source);
  if (
    actual.some((key) => !allowed.has(key)) ||
    requiredKeys.some((key) => !Object.prototype.hasOwnProperty.call(source, key))
  )
    invalid(`${context} has an unexpected field set`);
  return source;
}

export function textOf(value: unknown, max: number, context: string): string {
  if (typeof value !== 'string' || value.length > max)
    invalid(`${context} must be a string of at most ${max} characters`);
  return value;
}

export function nonBlankText(value: unknown, max: number, context: string): string {
  const result = textOf(value, max, context);
  if (!result.trim()) invalid(`${context} must not be blank`);
  return result;
}

export function choiceOf<T extends string>(
  value: unknown,
  allowed: readonly T[],
  context: string,
): T {
  if (typeof value !== 'string' || !allowed.includes(value as T))
    invalid(`${context} must be one of the era values`);
  return value as T;
}

export function booleanOf(value: unknown, context: string): boolean {
  if (typeof value !== 'boolean') invalid(`${context} must be a boolean`);
  return value;
}

export function numberAtLeast(value: unknown, min: number, context: string): number {
  if (typeof value !== 'number' || !Number.isFinite(value) || value < min)
    invalid(`${context} must be a finite number at least ${min}`);
  return value;
}

export function safeIntegerAtLeast(value: unknown, min: number, context: string): number {
  if (typeof value !== 'number' || !Number.isSafeInteger(value) || value < min)
    invalid(`${context} must be a safe integer at least ${min}`);
  return value;
}

export function timestampOf(value: unknown, context: string): string {
  const canonical = `${context} must be a canonical UTC ISO-8601 timestamp`;
  if (typeof value !== 'string') invalid(canonical);
  if (!/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z$/u.test(value)) invalid(canonical);
  const time = Date.parse(value);
  if (!Number.isFinite(time) || new Date(time).toISOString() !== value) invalid(canonical);
  return value;
}

export function timestampOrNull(value: unknown, context: string): string | null {
  return value === null ? null : timestampOf(value, context);
}

export function intervalOf(
  value: unknown,
  min: number,
  max: number,
  context: string,
): LegacySeriesScoreInterval {
  const source = recordFields(value, ['lower', 'upper'], [], context);
  const lower = source.lower;
  const upper = source.upper;
  if (
    typeof lower !== 'number' ||
    typeof upper !== 'number' ||
    !Number.isFinite(lower) ||
    !Number.isFinite(upper) ||
    lower > upper ||
    lower < min ||
    upper > max
  )
    invalid(`${context} must be a finite ordered interval within ${min}-${max}`);
  return { lower, upper };
}

export function intervalOrNull(
  value: unknown,
  min: number,
  max: number,
  context: string,
): LegacySeriesScoreInterval | null {
  return value === null ? null : intervalOf(value, min, max, context);
}

export function textList(value: unknown, context: string): readonly string[] {
  if (!Array.isArray(value) || value.length > listMax)
    invalid(`${context} must be a list of at most ${listMax} strings`);
  return value.map((item: unknown, index: number) =>
    nonBlankText(item, modelTextMax, `${context}[${index}]`),
  );
}

export function opaqueIdList(value: unknown, context: string): readonly string[] {
  if (!Array.isArray(value) || value.length > listMax)
    invalid(`${context} must be a list of at most ${listMax} strings`);
  return value.map((item: unknown, index: number) =>
    nonBlankText(item, idMax, `${context}[${index}]`),
  );
}

export function deepEqual(a: unknown, b: unknown): boolean {
  if (Object.is(a, b)) return true;
  if (typeof a !== typeof b || a === null || b === null || typeof a !== 'object') return false;
  if (Array.isArray(a) !== Array.isArray(b)) return false;
  if (Array.isArray(a) && Array.isArray(b)) {
    return a.length === b.length && a.every((item, index) => deepEqual(item, b[index]));
  }
  const aKeys = Object.keys(a);
  const bKeys = Object.keys(b as Record<string, unknown>);
  return (
    aKeys.length === bKeys.length &&
    aKeys.every(
      (key) =>
        Object.prototype.hasOwnProperty.call(b, key) &&
        deepEqual((a as Record<string, unknown>)[key], (b as Record<string, unknown>)[key]),
    )
  );
}

function participantPath(index: number): string {
  return index < 2 ? `result.people[${index}]` : 'result.control';
}

/**
 * Cross-reference validation across the decoded record: participant and trial
 * identity, copied trial/attempt integrity, trace plan and wall-time sums,
 * evidence references. Recorded scoring outputs are not recalculated.
 * Never recomputes BSC values; only checks the writer's own invariants.
 */
export function validateSeriesIntegrity(result: LegacySeriesResult): void {
  const participants: readonly LegacySeriesExperimentPerson[] = [
    result.people[0],
    result.people[1],
    result.control,
  ];

  if (
    result.people[0].profile.id !== 'a' ||
    result.people[1].profile.id !== 'b' ||
    result.control.profile.id !== 'neutral'
  )
    invalid("participants must use the writer-assigned profile ids 'a', 'b' and 'neutral'");

  validateTrialSequence(result);
  validateTrialPersonReferences(result);
  validateCopiedTrials(result, participants);
  validateEvidenceReferences(participants);
  validateTraces(result, participants);
  validateWallTimeSums(result, participants);

  if (result.status === 'complete') {
    for (const trial of result.trials) {
      if (trial.status !== 'evaluated')
        invalid(`a complete record cannot contain a '${trial.status}' trial`);
    }
  }
}

function validateTrialSequence(result: LegacySeriesResult): void {
  const expectedCount = result.protocol.repetitions * 3;
  if (result.trials.length !== expectedCount)
    invalid(`result.trials must list exactly ${expectedCount} trials in trial sequence`);
  result.trials.forEach((trial: LegacySeriesTrial, index: number) => {
    const round = Math.floor(index / 3) + 1;
    const offset = index % 3;
    const expectedPerson = result.protocol.participantOrder[round - 1]?.[offset];
    if (trial.personId !== expectedPerson) {
      if (!['a', 'b', 'neutral'].includes(trial.personId))
        invalid(
          `top-level trial at index ${index} references unknown participant '${trial.personId}'`,
        );
      invalid(
        `top-level trial at index ${index} breaks the trial sequence: expected participant '${expectedPerson}'`,
      );
    }
    if (trial.round !== round)
      invalid(
        `top-level trial at index ${index} breaks the trial sequence: expected round ${round}`,
      );
    const expectedId = `${round}-${expectedPerson}`;
    if (trial.id !== expectedId)
      invalid(`top-level trial at index ${index}: 'id' must equal '${expectedId}'`);
  });
}

function validateTrialPersonReferences(result: LegacySeriesResult): void {
  result.trials.forEach((trial, index) => {
    if (trial.score && trial.score.personId !== trial.personId)
      invalid(`result.trials[${index}] score 'person-id' mismatch with the trial person`);
    trial.evaluations.forEach((evaluation, evaluationIndex) => {
      if (evaluation.personId !== trial.personId)
        invalid(
          `result.trials[${index}].evaluations[${evaluationIndex}] 'person-id' mismatch with the trial person`,
        );
    });
  });
}

function validateCopiedTrials(
  result: LegacySeriesResult,
  participants: readonly LegacySeriesExperimentPerson[],
): void {
  participants.forEach((person: LegacySeriesExperimentPerson, index: number) => {
    if (person.score.personId !== person.profile.id)
      invalid(`${participantPath(index)} score 'person-id' mismatch with the profile id`);
    const own = result.trials.filter((trial) => trial.personId === person.profile.id);
    if (person.trials.length !== own.length)
      invalid(`${participantPath(index)} carries a conflicting copied trial set`);
    person.trials.forEach((trial, trialIndex) => {
      const topLevel = own[trialIndex];
      if (
        !topLevel ||
        trial.id !== topLevel.id ||
        trial.round !== topLevel.round ||
        trial.personId !== topLevel.personId
      )
        invalid(
          `${participantPath(index)}.trials[${trialIndex}] is a conflicting copied trial of an '${topLevel?.status}' trial`,
        );
      if (!deepEqual(trial, topLevel))
        invalid(
          `${participantPath(index)}.trials[${trialIndex}] is a conflicting copied trial of an '${topLevel.status}' trial`,
        );
    });
    const withAttempt = own.find((trial) => trial.attempt !== null);
    const expectedAttempt = withAttempt ? withAttempt.attempt : null;
    if (!deepEqual(person.attempt, expectedAttempt))
      invalid(`${participantPath(index)}.attempt is a conflicting copied attempt`);
  });
}

function validateEvidenceReferences(participants: readonly LegacySeriesExperimentPerson[]): void {
  participants.forEach((person: LegacySeriesExperimentPerson, index: number) => {
    const known = new Set((person.report?.evidence ?? []).map((item) => item.id));
    const rules = [
      ...person.baseProfile.rules,
      ...person.baseProfile.taskSkills,
      ...person.profile.rules,
      ...person.profile.taskSkills,
    ];
    for (const rule of rules) {
      for (const evidenceId of rule.evidenceIds) {
        if (!known.has(evidenceId))
          invalid(`${participantPath(index)} references unknown evidence id '${evidenceId}'`);
      }
    }
  });
}

function validateTraces(
  result: LegacySeriesResult,
  participants: readonly LegacySeriesExperimentPerson[],
): void {
  const byId = new Map(result.traces.map((entry) => [entry.id, entry]));
  const seen = new Set<string>();
  for (const entry of result.traces) {
    if (seen.has(entry.id)) invalid(`duplicate trace id '${entry.id}'`);
    seen.add(entry.id);
  }

  const requiredIds = [
    'rubric',
    'sources-0',
    'profile-0',
    'sources-1',
    'profile-1',
    'adapt-0',
    'adapt-1',
    'adapt-2',
  ];
  for (let round = 1; round <= result.protocol.repetitions; round++) {
    const roundTrials = result.trials.filter((trial) => trial.round === round);
    if (roundTrials.length > 0 && roundTrials.every((trial) => trial.status === 'evaluated')) {
      requiredIds.push(`judge-${round}-0`, `judge-${round}-1`);
    }
  }
  for (const id of requiredIds) {
    if (!byId.has(id)) invalid(`missing '${id}' trace`);
  }

  const participantIndex = new Map(participants.map((person, index) => [person.profile.id, index]));
  for (const trial of result.trials) {
    if (trial.status !== 'evaluated' && trial.status !== 'answered' && trial.status !== 'failed')
      continue;
    const id = `solve-${trial.id}`;
    const entry = byId.get(id);
    if (!entry) invalid(`missing '${id}' trace`);
    const expectedPerson = participantIndex.get(trial.personId);
    if (entry.stage !== 'person' || entry.person !== expectedPerson)
      invalid(
        `solve trace '${id}' trace person must be ${expectedPerson} for participant '${trial.personId}'`,
      );
  }
}

function validateWallTimeSums(
  result: LegacySeriesResult,
  participants: readonly LegacySeriesExperimentPerson[],
): void {
  participants.forEach((person: LegacySeriesExperimentPerson, index: number) => {
    let preparation = 0;
    let solution = 0;
    for (const entry of result.traces) {
      if (entry.person !== index) continue;
      if (entry.stage === 'person') solution += entry.trace.wallTimeMs;
      else preparation += entry.trace.wallTimeMs;
    }
    if (person.preparationWallTimeMs !== preparation)
      invalid(
        `${participantPath(index)} 'preparation-wall-time-ms' does not match the recorded trace wall-time sum`,
      );
    if (person.solutionWallTimeMs !== solution)
      invalid(
        `${participantPath(index)} 'solution-wall-time-ms' does not match the recorded trace wall-time sum`,
      );
  });
}
