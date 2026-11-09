import { decodeExecutionSnapshot } from '@proteus-int/contracts';
import type {
  EvaluationRubric,
  ExecutionSnapshot,
  ExperimentInput,
  ExperimentProtocol,
} from '@proteus-int/contracts';
import type { ExperimentPerson, ExperimentResult } from '../experiment/result';
import { validateV4References } from './result-v4-integrity';
import {
  decodeExperimentMemoryProtocol,
  decodePersonProfile,
  decodeStalkerReport,
} from './result-v4-profiles';
import {
  differenceInterval,
  decodeIntelligenceResult,
  decodeScoreSummary,
  decodeStageTrace,
  decodeTrial,
} from './result-v4-trials';
import {
  asArray,
  asBoolean,
  asChoice,
  asInteger,
  asInterval,
  asMap,
  asNumber,
  asRecord,
  asStringArray,
  asStringMap,
  asText,
  asTimestamp,
  checkEnvelopeSize,
  exactKeys,
  field,
  invalid,
  optionalField,
} from './result-v4-validation';

import type { StoredExportMetadata } from '../export/source-export-policy';
export type { StoredExportMetadata } from '../export/source-export-policy';

/** Decoded schemaVersion 4 saved result: the raw writer shape plus optional export metadata. */
export type StoredV4Result = ExperimentResult & {
  readonly exportMetadata?: StoredExportMetadata;
};

const topLevelKeys = [
  'schemaVersion',
  'scoringVersion',
  'status',
  'input',
  'protocol',
  'people',
  'control',
  'trials',
  'comparison',
  'traces',
  'finishedAt',
  'execution',
  'exportMetadata',
] as const;

const promptVersions = [
  'bsc-v2.2-source-style',
  'bsc-v2.3-source-identity',
  'bsc-v2.4-camel-case',
] as const;

function decodePersonInput(value: unknown, path: string) {
  const record = asRecord(value, path);
  exactKeys(record, ['label', 'context', 'publicUrls'], path);
  return {
    label: asText(field(record, 'label', path), `${path}.label`),
    context: asText(field(record, 'context', path), `${path}.context`),
    publicUrls: asStringArray(field(record, 'publicUrls', path), `${path}.publicUrls`),
  };
}

function decodeExperimentInput(value: unknown, path: string): ExperimentInput {
  const record = asRecord(value, path);
  exactKeys(record, ['task', 'people'], path);
  const people = asArray(field(record, 'people', path), `${path}.people`, 2);
  if (people.length !== 2) invalid(`${path}.people`, 'expected exactly two personas');
  return {
    task: asText(field(record, 'task', path), `${path}.task`),
    people: [
      decodePersonInput(people[0], `${path}.people[0]`),
      decodePersonInput(people[1], `${path}.people[1]`),
    ],
  };
}

function decodeRubricCriterion(value: unknown, path: string) {
  const record = asRecord(value, path);
  exactKeys(record, ['id', 'label', 'weight', 'required', 'anchors'], path);
  const weight = asNumber(field(record, 'weight', path), `${path}.weight`);
  if (weight <= 0) invalid(`${path}.weight`, 'expected a positive weight');
  const anchors = asRecord(field(record, 'anchors', path), `${path}.anchors`);
  exactKeys(anchors, ['unmet', 'partial', 'met'], `${path}.anchors`);
  return {
    id: asText(field(record, 'id', path), `${path}.id`),
    label: asText(field(record, 'label', path), `${path}.label`),
    weight,
    required: asBoolean(field(record, 'required', path), `${path}.required`),
    anchors: {
      unmet: asText(anchors.unmet, `${path}.anchors.unmet`),
      partial: asText(anchors.partial, `${path}.anchors.partial`),
      met: asText(anchors.met, `${path}.anchors.met`),
    },
  };
}

function decodeRubric(value: unknown, path: string): EvaluationRubric {
  const record = asRecord(value, path);
  exactKeys(record, ['version', 'criteria'], path);
  if (field(record, 'version', path) !== 2) invalid(`${path}.version`, 'expected version 2');
  const criteria = asArray(field(record, 'criteria', path), `${path}.criteria`, 12);
  if (criteria.length < 1) invalid(`${path}.criteria`, 'expected at least 1 criterion');
  return {
    version: 2,
    criteria: criteria.map((item, index) =>
      decodeRubricCriterion(item, `${path}.criteria[${index}]`),
    ),
  };
}

function decodeModelSetting(value: unknown, path: string) {
  const record = asRecord(value, path);
  exactKeys(record, ['provider', 'requestedModel', 'requestedEffort'], path);
  return {
    provider: asChoice(field(record, 'provider', path), ['codex', 'kimi'], `${path}.provider`),
    requestedModel: asText(field(record, 'requestedModel', path), `${path}.requestedModel`),
    requestedEffort: asChoice(
      field(record, 'requestedEffort', path),
      ['low', 'medium', 'high'],
      `${path}.requestedEffort`,
    ),
  };
}

function decodeProtocol(value: unknown, path: string): ExperimentProtocol {
  const record = asRecord(value, path);
  exactKeys(
    record,
    [
      'version',
      'task',
      'rubric',
      'repetitions',
      'participantOrder',
      'modelSettings',
      'formatAttempts',
      'stageTimeoutMs',
      'promptVersion',
      'createdAt',
      'memory',
      'sourceSnapshots',
    ],
    path,
  );
  if (field(record, 'version', path) !== 3)
    invalid(`${path}.version`, 'expected protocol version 3');
  if (field(record, 'formatAttempts', path) !== 2)
    invalid(`${path}.formatAttempts`, 'expected formatAttempts 2');
  if (field(record, 'stageTimeoutMs', path) !== 600000)
    invalid(`${path}.stageTimeoutMs`, 'expected stageTimeoutMs 600000');
  const repetitionsValue = field(record, 'repetitions', path);
  const repetitions: 1 | 3 =
    repetitionsValue === 1 || repetitionsValue === 3
      ? repetitionsValue
      : invalid(`${path}.repetitions`, 'expected 1 or 3 repetitions');
  const participantOrder = asArray(
    field(record, 'participantOrder', path),
    `${path}.participantOrder`,
    8,
  ).map((row, rowIndex) => {
    const ids = asArray(row, `${path}.participantOrder[${rowIndex}]`, 3);
    if (ids.length !== 3)
      invalid(`${path}.participantOrder[${rowIndex}]`, 'expected exactly three participant ids');
    return ids.map((id, idIndex) =>
      asText(id, `${path}.participantOrder[${rowIndex}][${idIndex}]`),
    );
  });
  if (participantOrder.length !== repetitions)
    invalid(`${path}.participantOrder`, 'expected one row per repetition');
  const decoded: ExperimentProtocol = {
    version: 3,
    task: asText(field(record, 'task', path), `${path}.task`),
    rubric: decodeRubric(field(record, 'rubric', path), `${path}.rubric`),
    repetitions,
    participantOrder,
    modelSettings: asMap(
      field(record, 'modelSettings', path),
      `${path}.modelSettings`,
      decodeModelSetting,
      16,
    ),
    formatAttempts: 2,
    stageTimeoutMs: 600000,
    promptVersion: asChoice(
      field(record, 'promptVersion', path),
      promptVersions,
      `${path}.promptVersion`,
    ),
    createdAt: asTimestamp(field(record, 'createdAt', path), `${path}.createdAt`),
  };
  const memory = optionalField(record, 'memory');
  const sourceSnapshots = optionalField(record, 'sourceSnapshots');
  return {
    ...decoded,
    ...(memory !== undefined
      ? { memory: decodeExperimentMemoryProtocol(memory, `${path}.memory`) }
      : {}),
    ...(sourceSnapshots !== undefined
      ? {
          sourceSnapshots: decodeSourceSnapshots(sourceSnapshots, `${path}.sourceSnapshots`),
        }
      : {}),
  };
}

function decodeSourceSnapshots(
  value: unknown,
  path: string,
): readonly [readonly string[], readonly string[]] {
  const snapshots = asArray(value, path, 2);
  if (snapshots.length !== 2) invalid(path, 'expected exactly two snapshot lists');
  return [asStringArray(snapshots[0], `${path}[0]`), asStringArray(snapshots[1], `${path}[1]`)];
}

function decodeExperimentPerson(value: unknown, path: string): ExperimentPerson {
  const record = asRecord(value, path);
  exactKeys(
    record,
    [
      'report',
      'baseProfile',
      'profile',
      'attempt',
      'score',
      'trials',
      'artifacts',
      'baselineDifference',
      'preparationWallTimeMs',
      'solutionWallTimeMs',
    ],
    path,
  );
  const report = field(record, 'report', path);
  const attempt = field(record, 'attempt', path);
  const baselineDifference = field(record, 'baselineDifference', path);
  return {
    report: report === null ? null : decodeStalkerReport(report, `${path}.report`),
    baseProfile: decodePersonProfile(field(record, 'baseProfile', path), `${path}.baseProfile`),
    profile: decodePersonProfile(field(record, 'profile', path), `${path}.profile`),
    attempt: attempt === null ? null : decodeIntelligenceResult(attempt, `${path}.attempt`),
    score: decodeScoreSummary(field(record, 'score', path), `${path}.score`),
    trials: asArray(field(record, 'trials', path), `${path}.trials`).map((item, index) =>
      decodeTrial(item, `${path}.trials[${index}]`),
    ),
    artifacts: asStringMap(field(record, 'artifacts', path), `${path}.artifacts`),
    baselineDifference:
      baselineDifference === null
        ? null
        : asInterval(baselineDifference, `${path}.baselineDifference`, differenceInterval),
    preparationWallTimeMs: asNumber(
      field(record, 'preparationWallTimeMs', path),
      `${path}.preparationWallTimeMs`,
      0,
    ),
    solutionWallTimeMs: asNumber(
      field(record, 'solutionWallTimeMs', path),
      `${path}.solutionWallTimeMs`,
      0,
    ),
  };
}

function decodeComparison(value: unknown, path: string): ExperimentResult['comparison'] {
  const record = asRecord(value, path);
  exactKeys(record, ['summary', 'difference', 'comparisonEligible'], path);
  const difference = field(record, 'difference', path);
  return {
    summary: asText(field(record, 'summary', path), `${path}.summary`),
    difference:
      difference === null ? null : asInterval(difference, `${path}.difference`, differenceInterval),
    comparisonEligible: asBoolean(
      field(record, 'comparisonEligible', path),
      `${path}.comparisonEligible`,
    ),
  };
}

function decodeExportMetadata(value: unknown, path: string): StoredExportMetadata {
  const record = asRecord(value, path);
  exactKeys(
    record,
    [
      'version',
      'redacted',
      'omittedStyleExemplars',
      'omittedSourceExcerpts',
      'preparationReasoningOmitted',
      'answerTextMayBeRedacted',
      'scoresReferenceOriginalAnswers',
      'limitations',
    ],
    path,
  );
  if (field(record, 'version', path) !== 1) invalid(`${path}.version`, 'expected version 1');
  if (field(record, 'scoresReferenceOriginalAnswers', path) !== true)
    invalid(`${path}.scoresReferenceOriginalAnswers`, 'expected true');
  return {
    version: 1,
    redacted: asBoolean(field(record, 'redacted', path), `${path}.redacted`),
    omittedStyleExemplars: asInteger(
      field(record, 'omittedStyleExemplars', path),
      `${path}.omittedStyleExemplars`,
      0,
    ),
    omittedSourceExcerpts: asInteger(
      field(record, 'omittedSourceExcerpts', path),
      `${path}.omittedSourceExcerpts`,
      0,
    ),
    preparationReasoningOmitted: asBoolean(
      field(record, 'preparationReasoningOmitted', path),
      `${path}.preparationReasoningOmitted`,
    ),
    answerTextMayBeRedacted: asBoolean(
      field(record, 'answerTextMayBeRedacted', path),
      `${path}.answerTextMayBeRedacted`,
    ),
    scoresReferenceOriginalAnswers: true,
    limitations: asStringArray(field(record, 'limitations', path), `${path}.limitations`),
  };
}

/**
 * Strict read-only decoder for saved schemaVersion 4 results. Accepts the raw
 * buildExperimentResult shape and the exportExperiment shape (with exportMetadata),
 * with or without the execution snapshot extension. Returns a fully detached value
 * or throws; no partial result, no v1-v3 conversion, no recomputation.
 */
export function decodeV4Result(value: unknown): StoredV4Result {
  checkEnvelopeSize(value);
  const record = asRecord(value, 'result');
  if (field(record, 'schemaVersion', 'result.schemaVersion') !== 4)
    invalid('result.schemaVersion', 'expected schemaVersion 4');
  if (field(record, 'scoringVersion', 'result.scoringVersion') !== 2)
    invalid('result.scoringVersion', 'expected scoringVersion 2');
  exactKeys(record, topLevelKeys, 'result');
  const status = asChoice(
    field(record, 'status', 'result.status'),
    ['complete', 'incomplete', 'cancelled'],
    'result.status',
  );
  const input = decodeExperimentInput(field(record, 'input', 'result'), 'result.input');
  const protocol = decodeProtocol(field(record, 'protocol', 'result'), 'result.protocol');
  const people = asArray(field(record, 'people', 'result'), 'result.people', 2);
  if (people.length !== 2) invalid('result.people', 'expected exactly two participants');
  const trials = asArray(field(record, 'trials', 'result'), 'result.trials');
  if (trials.length !== protocol.repetitions * 3)
    invalid('result.trials', `expected exactly ${protocol.repetitions * 3} planned trials`);
  const decodedTrials = trials.map((item, index) => decodeTrial(item, `result.trials[${index}]`));
  const decodedPeople = people.map((item, index) =>
    decodeExperimentPerson(item, `result.people[${index}]`),
  );
  const decoded: ExperimentResult = {
    schemaVersion: 4,
    scoringVersion: 2,
    status,
    input,
    protocol,
    people: decodedPeople,
    control: decodeExperimentPerson(field(record, 'control', 'result'), 'result.control'),
    trials: decodedTrials,
    comparison: decodeComparison(field(record, 'comparison', 'result'), 'result.comparison'),
    traces: asArray(field(record, 'traces', 'result'), 'result.traces', 64).map((item, index) =>
      decodeStageTrace(item, `result.traces[${index}]`),
    ),
    finishedAt: asTimestamp(field(record, 'finishedAt', 'result'), 'result.finishedAt'),
  };
  const execution = optionalField(record, 'execution');
  validateV4References(decoded);
  const exportMetadata = optionalField(record, 'exportMetadata');
  const withExecution: ExperimentResult =
    execution === undefined
      ? decoded
      : { ...decoded, execution: decodeExecution(execution, 'result.execution') };
  return exportMetadata === undefined
    ? withExecution
    : {
        ...withExecution,
        exportMetadata: decodeExportMetadata(exportMetadata, 'result.exportMetadata'),
      };
}

function decodeExecution(value: unknown, path: string): ExecutionSnapshot {
  try {
    const snapshot = decodeExecutionSnapshot(value);
    if (snapshot === null) invalid(path, 'expected a schemaVersion 1 execution snapshot');
    return snapshot;
  } catch (error) {
    if (error instanceof Error && error.message.startsWith('Invalid stored v4 result')) throw error;
    invalid(path, error instanceof Error ? error.message : 'invalid execution snapshot');
  }
}
