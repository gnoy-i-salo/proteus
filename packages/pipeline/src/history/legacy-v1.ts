export type LegacyV1Palette = 'turquoise' | 'violet';
export type LegacyV1Provider = 'codex' | 'kimi';
export type LegacyV1Effort = 'low' | 'medium' | 'high';
export type LegacyV1EvidenceKind = 'fact' | 'hypothesis';

export interface LegacyV1PersonInput {
  readonly label: string;
  readonly context: string;
  readonly publicUrls: readonly string[];
}

export interface LegacyV1ExperimentInput {
  readonly task: string;
  readonly people: readonly [LegacyV1PersonInput, LegacyV1PersonInput];
}

export interface LegacyV1Evidence {
  readonly id: string;
  readonly sourceUrl: string;
  readonly kind: LegacyV1EvidenceKind;
  readonly summary: string;
}

export interface LegacyV1StalkerReport {
  readonly evidence: readonly LegacyV1Evidence[];
  readonly estimatedBsc: number;
  readonly effort: LegacyV1Effort;
  readonly character: string;
  readonly thinkingStyle: string;
  readonly activity: string;
  readonly behaviorSummary: string;
}

export interface LegacyV1PersonProfile {
  readonly id: string;
  readonly label: string;
  readonly palette: LegacyV1Palette;
  readonly archetype: string;
  readonly skills: readonly string[];
}

export interface LegacyV1IntelligenceTrace {
  readonly provider: LegacyV1Provider;
  readonly reasoningMs: number;
  readonly terminalMs: number;
  readonly summary: string;
}

export interface LegacyV1Attempt {
  readonly answer: string;
  readonly trace: LegacyV1IntelligenceTrace;
}

export interface LegacyV1PersonScore {
  readonly personId: string;
  readonly reasoningMinutes: number;
  readonly bsc: number;
}

export interface LegacyV1ExperimentPerson {
  readonly report: LegacyV1StalkerReport;
  readonly profile: LegacyV1PersonProfile;
  readonly attempt: LegacyV1Attempt;
  readonly score: LegacyV1PersonScore;
  readonly artifacts: Readonly<Record<string, string>>;
}

export interface LegacyV1PersonaObservation {
  readonly strengths: readonly string[];
  readonly weaknesses: readonly string[];
  readonly observations: readonly string[];
}

export interface LegacyV1Comparison {
  readonly summary: string;
  readonly scores: readonly [LegacyV1PersonScore, LegacyV1PersonScore];
  readonly keyDifferences: readonly string[];
  readonly people: readonly [LegacyV1PersonaObservation, LegacyV1PersonaObservation];
  readonly ratio: number | null;
}

export interface LegacyV1Result {
  readonly input: LegacyV1ExperimentInput;
  readonly people: readonly [LegacyV1ExperimentPerson, LegacyV1ExperimentPerson];
  readonly comparison: LegacyV1Comparison;
  readonly finishedAt: string;
}

// Bounds taken from the actual v1 writer and contracts at 4e1c7c3: validateExperiment
// (task/context 4000, label 80, five URLs), collectPerson (30 evidence items, positive
// estimated-bsc), intelligence text()/texts() (24000 characters, 30 items), scorePerson
// (non-negative finite durations). The 8 MiB envelope, opaque URL text and the artifact
// map size are reader-side envelope guards for this boundary.
const maxResultBytes = 8 * 1024 * 1024;
const taskMax = 4000;
const labelMax = 80;
const contextMax = 4000;
const urlListMax = 5;
const modelTextMax = 24000;
const listMax = 30;
// The era writer bounded URL count, but not URL length. The result envelope
// bounds total memory; a new URL-specific limit would reject old valid files.
const urlTextMax = maxResultBytes;
const artifactCountMax = 256;
const artifactKeyMax = 512;
const artifactValueMax = 1024 * 1024;

function invalid(detail: string): never {
  throw new Error(`Invalid legacy v1 result: ${detail}`);
}

function recordFields(
  value: unknown,
  keys: readonly string[],
  context: string,
): Record<string, unknown> {
  if (!value || typeof value !== 'object' || Array.isArray(value))
    invalid(`${context} must be an object`);
  const source = value as Record<string, unknown>;
  const actual = Object.keys(source);
  if (actual.length !== keys.length || actual.some((key) => !keys.includes(key)))
    invalid(`${context} has an unexpected field set`);
  return source;
}

function textOf(value: unknown, max: number, context: string): string {
  if (typeof value !== 'string' || value.length > max)
    invalid(`${context} must be a string of at most ${max} characters`);
  return value;
}

function nonBlankText(value: unknown, max: number, context: string): string {
  const result = textOf(value, max, context);
  if (!result.trim()) invalid(`${context} must not be blank`);
  return result;
}

function numberAtLeast(value: unknown, min: number, context: string): number {
  if (typeof value !== 'number' || !Number.isFinite(value) || value < min)
    invalid(`${context} must be a finite number at least ${min}`);
  return value;
}

function positiveNumber(value: unknown, context: string): number {
  if (typeof value !== 'number' || !Number.isFinite(value) || value <= 0)
    invalid(`${context} must be a finite number above 0`);
  return value;
}

function choiceOf<T extends string>(value: unknown, allowed: readonly T[], context: string): T {
  if (typeof value !== 'string' || !allowed.includes(value as T))
    invalid(`${context} must be one of the era values`);
  return value as T;
}

function textList(value: unknown, context: string): readonly string[] {
  if (!Array.isArray(value) || value.length > listMax)
    invalid(`${context} must be a list of at most ${listMax} strings`);
  return value.map((item: unknown, index: number) =>
    nonBlankText(item, modelTextMax, `${context}[${index}]`),
  );
}

function timestampOf(value: unknown): string {
  const result = textOf(value, modelTextMax, "result 'finished-at'");
  if (
    !/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z$/u.test(result) ||
    !Number.isFinite(Date.parse(result)) ||
    new Date(result).toISOString() !== result
  )
    invalid("result 'finished-at' must be an ISO-8601 UTC timestamp");
  return result;
}

function pair<T>(
  value: unknown,
  decode: (item: unknown, context: string) => T,
  context: string,
): readonly [T, T] {
  if (!Array.isArray(value) || value.length !== 2)
    invalid(`${context} must contain exactly two entries`);
  return [decode(value[0], `${context}[0]`), decode(value[1], `${context}[1]`)];
}

function decodeEvidence(value: unknown, context: string): LegacyV1Evidence {
  const source = recordFields(value, ['id', 'source-url', 'kind', 'summary'], context);
  return {
    id: nonBlankText(source['id'], modelTextMax, `${context}.id`),
    sourceUrl: textOf(source['source-url'], urlTextMax, `${context}.source-url`),
    kind: choiceOf(source['kind'], ['fact', 'hypothesis'] as const, `${context}.kind`),
    summary: nonBlankText(source['summary'], modelTextMax, `${context}.summary`),
  };
}

function decodeReport(value: unknown, context: string): LegacyV1StalkerReport {
  const source = recordFields(
    value,
    [
      'evidence',
      'estimated-bsc',
      'effort',
      'character',
      'thinking-style',
      'activity',
      'behavior-summary',
    ],
    context,
  );
  if (!Array.isArray(source['evidence']) || source['evidence'].length > listMax)
    invalid(`${context}.evidence must be a list of at most ${listMax} items`);
  return {
    evidence: source['evidence'].map((item: unknown, index: number) =>
      decodeEvidence(item, `${context}.evidence[${index}]`),
    ),
    estimatedBsc: positiveNumber(source['estimated-bsc'], `${context}.estimated-bsc`),
    effort: choiceOf(source['effort'], ['low', 'medium', 'high'] as const, `${context}.effort`),
    character: nonBlankText(source['character'], modelTextMax, `${context}.character`),
    thinkingStyle: nonBlankText(
      source['thinking-style'],
      modelTextMax,
      `${context}.thinking-style`,
    ),
    activity: nonBlankText(source['activity'], modelTextMax, `${context}.activity`),
    behaviorSummary: nonBlankText(
      source['behavior-summary'],
      modelTextMax,
      `${context}.behavior-summary`,
    ),
  };
}

function decodeProfile(value: unknown, context: string): LegacyV1PersonProfile {
  const source = recordFields(value, ['id', 'label', 'palette', 'archetype', 'skills'], context);
  const skills = textList(source['skills'], `${context}.skills`);
  if (skills.length < 1) invalid(`${context}.skills must contain at least one skill`);
  return {
    id: nonBlankText(source['id'], modelTextMax, `${context}.id`),
    label: nonBlankText(source['label'], labelMax, `${context}.label`),
    palette: choiceOf(source['palette'], ['turquoise', 'violet'] as const, `${context}.palette`),
    archetype: nonBlankText(source['archetype'], modelTextMax, `${context}.archetype`),
    skills,
  };
}

function decodeTrace(value: unknown, context: string): LegacyV1IntelligenceTrace {
  const source = recordFields(
    value,
    ['provider', 'reasoning-ms', 'terminal-ms', 'summary'],
    context,
  );
  return {
    provider: choiceOf(source['provider'], ['codex', 'kimi'] as const, `${context}.provider`),
    reasoningMs: numberAtLeast(source['reasoning-ms'], 0, `${context}.reasoning-ms`),
    terminalMs: numberAtLeast(source['terminal-ms'], 0, `${context}.terminal-ms`),
    summary: nonBlankText(source['summary'], modelTextMax, `${context}.summary`),
  };
}

function decodeAttempt(value: unknown, context: string): LegacyV1Attempt {
  const source = recordFields(value, ['answer', 'trace'], context);
  return {
    answer: nonBlankText(source['answer'], modelTextMax, `${context}.answer`),
    trace: decodeTrace(source['trace'], `${context}.trace`),
  };
}

function decodeScore(value: unknown, context: string): LegacyV1PersonScore {
  const source = recordFields(value, ['person-id', 'reasoning-minutes', 'bsc'], context);
  return {
    personId: nonBlankText(source['person-id'], modelTextMax, `${context}.person-id`),
    reasoningMinutes: numberAtLeast(source['reasoning-minutes'], 0, `${context}.reasoning-minutes`),
    bsc: numberAtLeast(source['bsc'], 0, `${context}.bsc`),
  };
}

function decodeArtifacts(value: unknown, context: string): Readonly<Record<string, string>> {
  if (!value || typeof value !== 'object' || Array.isArray(value))
    invalid(`${context} must be an object keyed by artifact names`);
  const source = value as Record<string, unknown>;
  const entries = Object.entries(source);
  if (entries.length > artifactCountMax)
    invalid(`${context} has more than ${artifactCountMax} artifacts`);
  return Object.fromEntries(
    entries.map(([name, content]) => {
      if (name.length === 0 || name.length > artifactKeyMax)
        invalid(`${context} has artifact names of 1 to ${artifactKeyMax} characters`);
      return [name, textOf(content, artifactValueMax, `${context}[${JSON.stringify(name)}]`)];
    }),
  );
}

function decodePerson(value: unknown, context: string): LegacyV1ExperimentPerson {
  const source = recordFields(
    value,
    ['report', 'profile', 'attempt', 'score', 'artifacts'],
    context,
  );
  return {
    report: decodeReport(source['report'], `${context}.report`),
    profile: decodeProfile(source['profile'], `${context}.profile`),
    attempt: decodeAttempt(source['attempt'], `${context}.attempt`),
    score: decodeScore(source['score'], `${context}.score`),
    artifacts: decodeArtifacts(source['artifacts'], `${context}.artifacts`),
  };
}

function decodePersonInput(value: unknown, context: string): LegacyV1PersonInput {
  const source = recordFields(value, ['label', 'context', 'public-urls'], context);
  if (!Array.isArray(source['public-urls']) || source['public-urls'].length > urlListMax)
    invalid(`${context}.public-urls must be a list of at most ${urlListMax} strings`);
  return {
    label: nonBlankText(source['label'], labelMax, `${context}.label`),
    context: textOf(source['context'], contextMax, `${context}.context`),
    publicUrls: source['public-urls'].map((item: unknown, index: number) =>
      textOf(item, urlTextMax, `${context}.public-urls[${index}]`),
    ),
  };
}

function decodeInput(value: unknown): LegacyV1ExperimentInput {
  const source = recordFields(value, ['task', 'people'], 'result.input');
  return {
    task: nonBlankText(source['task'], taskMax, 'result.input.task'),
    people: pair(source['people'], decodePersonInput, 'result.input.people'),
  };
}

function decodeObservation(value: unknown, context: string): LegacyV1PersonaObservation {
  const source = recordFields(value, ['strengths', 'weaknesses', 'observations'], context);
  return {
    strengths: textList(source['strengths'], `${context}.strengths`),
    weaknesses: textList(source['weaknesses'], `${context}.weaknesses`),
    observations: textList(source['observations'], `${context}.observations`),
  };
}

function decodeComparison(value: unknown): LegacyV1Comparison {
  const context = 'result.comparison';
  const source = recordFields(
    value,
    ['summary', 'scores', 'key-differences', 'people', 'ratio'],
    context,
  );
  return {
    summary: nonBlankText(source['summary'], modelTextMax, `${context}.summary`),
    scores: pair(source['scores'], decodeScore, `${context}.scores`),
    keyDifferences: textList(source['key-differences'], `${context}.key-differences`),
    people: pair(source['people'], decodeObservation, `${context}.people`),
    ratio: source['ratio'] === null ? null : numberAtLeast(source['ratio'], 1, `${context}.ratio`),
  };
}

function checkEnvelope(value: unknown): void {
  let bytes = 0;
  try {
    bytes = new TextEncoder().encode(JSON.stringify(value) ?? '').length;
  } catch {
    invalid('input is not JSON-serializable');
  }
  if (bytes > maxResultBytes) invalid('result exceeds the 8 MiB file envelope');
}

export function decodeLegacyV1Result(value: unknown): LegacyV1Result {
  checkEnvelope(value);
  const source = recordFields(value, ['input', 'people', 'comparison', 'finished-at'], 'result');
  return {
    input: decodeInput(source['input']),
    people: pair(source['people'], decodePerson, 'result.people'),
    comparison: decodeComparison(source['comparison']),
    finishedAt: timestampOf(source['finished-at']),
  };
}
