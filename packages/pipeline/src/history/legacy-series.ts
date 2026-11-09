// Bounded reader for the exact legacy series-result schemas written by the
// pinned v2 (schema-version 2, prompt bsc-v2.1) and v3 (schema-version 3,
// prompts bsc-v2.2-source-style / bsc-v2.3-source-identity) writers, both at
// scoring-version 2. Decoding is strict and per-object: exact key sets per
// schema, era-mismatched and unknown keys rejected, canonical UTC timestamps,
// finite ordered intervals, and the writer's own cross-reference invariants.
// Recorded BSC values, redactions and all source/answer/reasoning text are
// preserved verbatim; nothing is scored, re-generated or un-redacted here.
import type {
  LegacySeriesComparison,
  LegacySeriesDecodeContext,
  LegacySeriesExperimentPerson,
  LegacySeriesResult,
  LegacySeriesSchemaVersion,
  LegacySeriesTrial,
} from './legacy-series-types';
import {
  decodeArtifacts,
  decodeExportMetadata,
  decodeInput,
  decodePersonProfile,
  decodeReport,
} from './legacy-series-profiles';
import {
  decodeAttempt,
  decodeProtocol,
  decodeScoreSummary,
  decodeStageTrace,
  decodeTrial,
} from './legacy-series-trials';
import {
  booleanOf,
  choiceOf,
  intervalOrNull,
  invalid,
  maxResultBytes,
  modelTextMax,
  nonBlankText,
  numberAtLeast,
  recordFields,
  timestampOf,
  validateSeriesIntegrity,
} from './legacy-series-validation';

const topLevelKeys = [
  'schema-version',
  'scoring-version',
  'status',
  'input',
  'protocol',
  'people',
  'control',
  'trials',
  'comparison',
  'traces',
  'finished-at',
] as const;

function checkEnvelope(value: unknown): void {
  let bytes = 0;
  try {
    bytes = new TextEncoder().encode(JSON.stringify(value) ?? '').length;
  } catch {
    invalid('input is not JSON-serializable');
  }
  if (bytes > maxResultBytes) invalid('result exceeds the 8 MiB file envelope');
}

function decodeComparison(value: unknown): LegacySeriesComparison {
  const source = recordFields(
    value,
    ['summary', 'difference', 'comparison-eligible'],
    [],
    'result.comparison',
  );
  return {
    summary: nonBlankText(source.summary, modelTextMax, 'result.comparison.summary'),
    difference: intervalOrNull(source.difference, -100, 100, 'result.comparison.difference'),
    comparisonEligible: booleanOf(
      source['comparison-eligible'],
      'result.comparison.comparison-eligible',
    ),
  };
}

function decodeParticipant(
  value: unknown,
  context: LegacySeriesDecodeContext,
  contextPath: string,
): LegacySeriesExperimentPerson {
  const source = recordFields(
    value,
    [
      'report',
      'base-profile',
      'profile',
      'attempt',
      'score',
      'trials',
      'artifacts',
      'baseline-difference',
      'preparation-wall-time-ms',
      'solution-wall-time-ms',
    ],
    [],
    contextPath,
  );
  if (!Array.isArray(source.trials))
    invalid(`${contextPath}.trials must be a list of copied trial results`);
  return {
    report:
      source.report === null ? null : decodeReport(source.report, context, `${contextPath}.report`),
    baseProfile: decodePersonProfile(
      source['base-profile'],
      context,
      `${contextPath}.base-profile`,
    ),
    profile: decodePersonProfile(source.profile, context, `${contextPath}.profile`),
    attempt:
      source.attempt === null
        ? null
        : decodeAttempt(source.attempt, context, `${contextPath}.attempt`),
    score: decodeScoreSummary(source.score, context.repetitions, `${contextPath}.score`),
    trials: (source.trials as unknown[]).map((item, index) =>
      decodeTrial(item, context, `${contextPath}.trials[${index}]`),
    ),
    artifacts: decodeArtifacts(source.artifacts, `${contextPath}.artifacts`),
    baselineDifference: intervalOrNull(
      source['baseline-difference'],
      -100,
      100,
      `${contextPath}.baseline-difference`,
    ),
    preparationWallTimeMs: numberAtLeast(
      source['preparation-wall-time-ms'],
      0,
      `${contextPath}.preparation-wall-time-ms`,
    ),
    solutionWallTimeMs: numberAtLeast(
      source['solution-wall-time-ms'],
      0,
      `${contextPath}.solution-wall-time-ms`,
    ),
  };
}

function decodeTrials(
  value: unknown,
  context: LegacySeriesDecodeContext,
): readonly LegacySeriesTrial[] {
  if (!Array.isArray(value)) invalid('result.trials must be a list of trial results');
  return (value as unknown[]).map((item, index) =>
    decodeTrial(item, context, `result.trials[${index}]`),
  );
}

function decodeTraces(value: unknown) {
  if (!Array.isArray(value)) invalid('result.traces must be a list of stage traces');
  return (value as unknown[]).map((item, index) =>
    decodeStageTrace(item, `result.traces[${index}]`),
  );
}

function readSchemaVersion(source: Record<string, unknown>): LegacySeriesSchemaVersion {
  const keys = Object.keys(source);
  const hasAllBaseKeys = topLevelKeys.every((key) =>
    Object.prototype.hasOwnProperty.call(source, key),
  );
  const onlyAllowedKeys = keys.every(
    (key) => (topLevelKeys as readonly string[]).includes(key) || key === 'export-metadata',
  );
  if (!hasAllBaseKeys || !onlyAllowedKeys) invalid('result has an unexpected field set');
  const schemaVersion = source['schema-version'];
  if (schemaVersion !== 2 && schemaVersion !== 3) invalid("result 'schema-version' must be 2 or 3");
  if (schemaVersion === 2 && Object.prototype.hasOwnProperty.call(source, 'export-metadata'))
    invalid('result has an unexpected field set');
  return schemaVersion;
}

export function decodeLegacySeriesResult(value: unknown): LegacySeriesResult {
  checkEnvelope(value);
  if (!value || typeof value !== 'object' || Array.isArray(value))
    invalid('result must be an object');
  const source = value as Record<string, unknown>;

  const schemaVersion = readSchemaVersion(source);
  const status = choiceOf(
    source.status,
    ['complete', 'incomplete', 'cancelled'] as const,
    "result 'status'",
  );
  if (source['scoring-version'] !== 2) invalid("result 'scoring-version' must be 2");

  const input = decodeInput(source.input);
  const protocol = decodeProtocol(source.protocol, { schemaVersion });
  const context: LegacySeriesDecodeContext = {
    schemaVersion,
    repetitions: protocol.repetitions,
    rubricCriteria: protocol.rubric.criteria,
  };

  if (!Array.isArray(source.people) || source.people.length !== 2)
    invalid('result.people must list exactly two people');
  const people: readonly [LegacySeriesExperimentPerson, LegacySeriesExperimentPerson] = [
    decodeParticipant(source.people[0], context, 'result.people[0]'),
    decodeParticipant(source.people[1], context, 'result.people[1]'),
  ];
  const control = decodeParticipant(source.control, context, 'result.control');
  const trials = decodeTrials(source.trials, context);
  const comparison = decodeComparison(source.comparison);
  const traces = decodeTraces(source.traces);
  const finishedAt = timestampOf(source['finished-at'], "result 'finished-at'");

  const result: LegacySeriesResult = {
    schemaVersion,
    scoringVersion: 2,
    status,
    input,
    protocol,
    people,
    control,
    trials,
    comparison,
    traces,
    finishedAt,
  };
  if (schemaVersion === 3 && Object.prototype.hasOwnProperty.call(source, 'export-metadata')) {
    Object.assign(result, { exportMetadata: decodeExportMetadata(source['export-metadata']) });
  }

  validateSeriesIntegrity(result);
  return result;
}
