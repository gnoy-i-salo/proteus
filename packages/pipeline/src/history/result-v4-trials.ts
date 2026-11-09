import { decodeExecutionSnapshot } from '@proteus-int/contracts';
import type {
  AnswerEvaluation,
  CriterionAssessment,
  CriterionScore,
  IntelligenceResult,
  IntelligenceTrace,
  InvocationAttempt,
  ScoreSummary,
  StageLifecycle,
  TrialScore,
} from '@proteus-int/contracts';
import type { StageTrace } from '../execution/types';
import type { TrialResult } from '../experiment/result';
import {
  asArray,
  asBoolean,
  asChoice,
  asInteger,
  asInterval,
  asNullableInterval,
  asNullableText,
  asNumber,
  asRecord,
  asStringArray,
  asText,
  exactKeys,
  field,
  invalid,
  optionalField,
  type IntervalScale,
} from './result-v4-validation';

export const criterionInterval: IntervalScale = { min: 0, max: 1 };
export const scoreInterval: IntervalScale = { min: 0, max: 100 };
export const differenceInterval: IntervalScale = { min: -100, max: 100 };

const providers = ['codex', 'kimi'] as const;
const efforts = ['low', 'medium', 'high'] as const;
const components = ['stalker', 'person-fabric', 'person', 'brain-analyzator'] as const;
const criterionStatuses = ['unmet', 'partial', 'met', 'unknown'] as const;
const requiredStatuses = ['passed', 'failed', 'unknown'] as const;
export function decodeIntelligenceTrace(value: unknown, path: string): IntelligenceTrace {
  const record = asRecord(value, path);
  exactKeys(
    record,
    [
      'provider',
      'reasoningMs',
      'wallTimeMs',
      'activeReasoningMs',
      'requestedModel',
      'resolvedModel',
      'requestedEffort',
      'appliedEffort',
      'cliVersion',
      'terminalMs',
      'summary',
      'reasoning',
      'reasoningTruncated',
    ],
    path,
  );
  const decoded: IntelligenceTrace = {
    provider: asChoice(field(record, 'provider', path), providers, `${path}.provider`),
    reasoningMs: asNumber(field(record, 'reasoningMs', path), `${path}.reasoningMs`, 0),
    terminalMs: asNumber(field(record, 'terminalMs', path), `${path}.terminalMs`, 0),
    summary: asText(field(record, 'summary', path), `${path}.summary`),
  };
  const wallTimeMs = optionalField(record, 'wallTimeMs');
  const activeReasoningMs = optionalField(record, 'activeReasoningMs');
  const requestedModel = optionalField(record, 'requestedModel');
  const resolvedModel = optionalField(record, 'resolvedModel');
  const requestedEffort = optionalField(record, 'requestedEffort');
  const appliedEffort = optionalField(record, 'appliedEffort');
  const cliVersion = optionalField(record, 'cliVersion');
  const reasoning = optionalField(record, 'reasoning');
  const reasoningTruncated = optionalField(record, 'reasoningTruncated');
  return {
    ...decoded,
    ...(wallTimeMs !== undefined
      ? { wallTimeMs: asNumber(wallTimeMs, `${path}.wallTimeMs`, 0) }
      : {}),
    ...(activeReasoningMs !== undefined
      ? {
          activeReasoningMs:
            activeReasoningMs === null
              ? null
              : invalid(`${path}.activeReasoningMs`, 'expected null'),
        }
      : {}),
    ...(requestedModel !== undefined
      ? { requestedModel: asText(requestedModel, `${path}.requestedModel`) }
      : {}),
    ...(resolvedModel !== undefined
      ? { resolvedModel: asNullableText(resolvedModel, `${path}.resolvedModel`) }
      : {}),
    ...(requestedEffort !== undefined
      ? { requestedEffort: asChoice(requestedEffort, efforts, `${path}.requestedEffort`) }
      : {}),
    ...(appliedEffort !== undefined
      ? {
          appliedEffort:
            appliedEffort === null
              ? null
              : asChoice(appliedEffort, efforts, `${path}.appliedEffort`),
        }
      : {}),
    ...(cliVersion !== undefined
      ? { cliVersion: asNullableText(cliVersion, `${path}.cliVersion`) }
      : {}),
    ...(reasoning !== undefined
      ? {
          reasoning: asArray(reasoning, `${path}.reasoning`).map((item, index) => {
            const block = asRecord(item, `${path}.reasoning[${index}]`);
            exactKeys(block, ['id', 'kind', 'text'], `${path}.reasoning[${index}]`);
            return {
              id: asText(block.id, `${path}.reasoning[${index}].id`),
              kind: asChoice(
                block.kind,
                ['summary', 'thinking'],
                `${path}.reasoning[${index}].kind`,
              ),
              text: asText(block.text, `${path}.reasoning[${index}].text`),
            };
          }),
        }
      : {}),
    ...(reasoningTruncated !== undefined
      ? { reasoningTruncated: asBoolean(reasoningTruncated, `${path}.reasoningTruncated`) }
      : {}),
  };
}

export function decodeIntelligenceResult(value: unknown, path: string): IntelligenceResult {
  const record = asRecord(value, path);
  exactKeys(record, ['invocationId', 'answer', 'trace', 'reply'], path);
  const decoded: IntelligenceResult = {
    answer: asText(field(record, 'answer', path), `${path}.answer`),
    trace: decodeIntelligenceTrace(field(record, 'trace', path), `${path}.trace`),
  };
  const invocationId = optionalField(record, 'invocationId');
  const reply = optionalField(record, 'reply');
  return {
    ...decoded,
    ...(invocationId !== undefined
      ? { invocationId: asText(invocationId, `${path}.invocationId`) }
      : {}),
    ...(reply !== undefined
      ? {
          reply: (() => {
            const replyPath = `${path}.reply`;
            const replyRecord = asRecord(reply, replyPath);
            exactKeys(replyRecord, ['version', 'messages', 'styleConflicts'], replyPath);
            if (field(replyRecord, 'version', replyPath) !== 2)
              invalid(`${replyPath}.version`, 'expected version 2');
            return {
              version: 2 as const,
              messages: asStringArray(
                field(replyRecord, 'messages', replyPath),
                `${replyPath}.messages`,
              ),
              styleConflicts: asStringArray(
                field(replyRecord, 'styleConflicts', replyPath),
                `${replyPath}.styleConflicts`,
              ),
            };
          })(),
        }
      : {}),
  };
}

export function decodeInvocationAttempt(value: unknown, path: string): InvocationAttempt {
  const record = asRecord(value, path);
  exactKeys(record, ['index', 'status', 'wallTimeMs', 'error'], path);
  return {
    index: asInteger(field(record, 'index', path), `${path}.index`, 1),
    status: asChoice(
      field(record, 'status', path),
      ['returned', 'format-rejected', 'accepted', 'failed'],
      `${path}.status`,
    ),
    wallTimeMs: asNumber(field(record, 'wallTimeMs', path), `${path}.wallTimeMs`, 0),
    error: asNullableText(field(record, 'error', path), `${path}.error`),
  };
}

function decodeCriterionAssessment(value: unknown, path: string): CriterionAssessment {
  const record = asRecord(value, path);
  exactKeys(record, ['criterionId', 'status', 'reason', 'quote'], path);
  return {
    criterionId: asText(field(record, 'criterionId', path), `${path}.criterionId`),
    status: asChoice(field(record, 'status', path), criterionStatuses, `${path}.status`),
    reason: asText(field(record, 'reason', path), `${path}.reason`),
    quote: asText(field(record, 'quote', path), `${path}.quote`),
  };
}

function decodeAnswerEvaluation(value: unknown, path: string): AnswerEvaluation {
  const record = asRecord(value, path);
  exactKeys(record, ['personId', 'criteria'], path);
  return {
    personId: asText(field(record, 'personId', path), `${path}.personId`),
    criteria: asArray(field(record, 'criteria', path), `${path}.criteria`).map((item, index) =>
      decodeCriterionAssessment(item, `${path}.criteria[${index}]`),
    ),
  };
}

function decodeCriterionScore(value: unknown, path: string): CriterionScore {
  const record = asRecord(value, path);
  exactKeys(record, ['criterionId', 'interval', 'assessments', 'disputed'], path);
  return {
    criterionId: asText(field(record, 'criterionId', path), `${path}.criterionId`),
    interval: asInterval(field(record, 'interval', path), `${path}.interval`, criterionInterval),
    assessments: asArray(field(record, 'assessments', path), `${path}.assessments`).map(
      (item, index) => decodeCriterionAssessment(item, `${path}.assessments[${index}]`),
    ),
    disputed: asBoolean(field(record, 'disputed', path), `${path}.disputed`),
  };
}

export function decodeTrialScore(value: unknown, path: string): TrialScore {
  const record = asRecord(value, path);
  exactKeys(
    record,
    ['version', 'personId', 'interval', 'criteria', 'requiredStatus', 'judgeDisagreement'],
    path,
  );
  if (field(record, 'version', path) !== 2) invalid(`${path}.version`, 'expected version 2');
  return {
    version: 2,
    personId: asText(field(record, 'personId', path), `${path}.personId`),
    interval: asInterval(field(record, 'interval', path), `${path}.interval`, scoreInterval),
    criteria: asArray(field(record, 'criteria', path), `${path}.criteria`).map((item, index) =>
      decodeCriterionScore(item, `${path}.criteria[${index}]`),
    ),
    requiredStatus: asChoice(
      field(record, 'requiredStatus', path),
      requiredStatuses,
      `${path}.requiredStatus`,
    ),
    judgeDisagreement: asBoolean(
      field(record, 'judgeDisagreement', path),
      `${path}.judgeDisagreement`,
    ),
  };
}

export function decodeScoreSummary(value: unknown, path: string): ScoreSummary {
  const record = asRecord(value, path);
  exactKeys(
    record,
    [
      'version',
      'personId',
      'interval',
      'point',
      'planned',
      'evaluated',
      'complete',
      'spread',
      'requiredStatus',
      'judgeDisagreement',
      'comparisonEligible',
    ],
    path,
  );
  if (field(record, 'version', path) !== 2) invalid(`${path}.version`, 'expected version 2');
  return {
    version: 2,
    personId: asText(field(record, 'personId', path), `${path}.personId`),
    interval: asNullableInterval(
      field(record, 'interval', path),
      `${path}.interval`,
      scoreInterval,
    ),
    point:
      field(record, 'point', path) === null
        ? null
        : asNumber(field(record, 'point', path), `${path}.point`, 0, 100),
    planned: asInteger(field(record, 'planned', path), `${path}.planned`, 0),
    evaluated: asInteger(field(record, 'evaluated', path), `${path}.evaluated`, 0),
    complete: asBoolean(field(record, 'complete', path), `${path}.complete`),
    spread: asNullableInterval(field(record, 'spread', path), `${path}.spread`, scoreInterval),
    requiredStatus: asChoice(
      field(record, 'requiredStatus', path),
      requiredStatuses,
      `${path}.requiredStatus`,
    ),
    judgeDisagreement: asBoolean(
      field(record, 'judgeDisagreement', path),
      `${path}.judgeDisagreement`,
    ),
    comparisonEligible: asBoolean(
      field(record, 'comparisonEligible', path),
      `${path}.comparisonEligible`,
    ),
  };
}

export function decodeTrial(value: unknown, path: string): TrialResult {
  const record = asRecord(value, path);
  exactKeys(
    record,
    ['id', 'round', 'personId', 'status', 'attempt', 'score', 'evaluations', 'error'],
    path,
  );
  const attempt = field(record, 'attempt', path);
  const score = field(record, 'score', path);
  return {
    id: asText(field(record, 'id', path), `${path}.id`),
    round: asInteger(field(record, 'round', path), `${path}.round`, 1),
    personId: asText(field(record, 'personId', path), `${path}.personId`),
    status: asChoice(
      field(record, 'status', path),
      ['pending', 'answered', 'evaluated', 'failed', 'skipped'],
      `${path}.status`,
    ),
    attempt: attempt === null ? null : decodeIntelligenceResult(attempt, `${path}.attempt`),
    score: score === null ? null : decodeTrialScore(score, `${path}.score`),
    evaluations: asArray(field(record, 'evaluations', path), `${path}.evaluations`).map(
      (item, index) => decodeAnswerEvaluation(item, `${path}.evaluations[${index}]`),
    ),
    error: asNullableText(field(record, 'error', path), `${path}.error`),
  };
}

// Reuse the owning lifecycle decoder for a standalone observed stage. The
// temporary envelope only supplies its recorded run ID; it is never returned.
function decodeStageLifecycle(value: unknown, path: string): StageLifecycle {
  const record = asRecord(value, path);
  const snapshot = decodeExecutionSnapshot({
    schemaVersion: 1,
    runId: field(record, 'runId', path),
    stages: [record],
  });
  return snapshot!.stages[0]!;
}

export function decodeStageTrace(value: unknown, path: string): StageTrace {
  const record = asRecord(value, path);
  exactKeys(record, ['id', 'stage', 'person', 'label', 'trace', 'attempts', 'lifecycle'], path);
  const person = field(record, 'person', path);
  const decoded: StageTrace = {
    id: asText(field(record, 'id', path), `${path}.id`),
    stage: asChoice(field(record, 'stage', path), components, `${path}.stage`),
    person: person === null ? null : asInteger(person, `${path}.person`, 0, 2),
    label: asText(field(record, 'label', path), `${path}.label`),
    trace: decodeIntelligenceTrace(field(record, 'trace', path), `${path}.trace`),
    attempts: asArray(field(record, 'attempts', path), `${path}.attempts`).map((item, index) =>
      decodeInvocationAttempt(item, `${path}.attempts[${index}]`),
    ),
  };
  const lifecycle = optionalField(record, 'lifecycle');
  return lifecycle === undefined
    ? decoded
    : { ...decoded, lifecycle: decodeStageLifecycle(lifecycle, `${path}.lifecycle`) };
}
