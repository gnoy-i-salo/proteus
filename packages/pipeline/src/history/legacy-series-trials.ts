// Per-object decoders for the protocol, trace, attempt/reply, trial and BSC v2
// scoring shapes of the exact legacy series-result schemas. Recorded scores are
// validated for internal consistency but never recomputed or re-scored.
import type {
  LegacySeriesAnswerEvaluation,
  LegacySeriesAttempt,
  LegacySeriesCriterionAssessment,
  LegacySeriesCriterionScore,
  LegacySeriesDecodeContext,
  LegacySeriesIntelligenceTrace,
  LegacySeriesInvocationAttempt,
  LegacySeriesModelSetting,
  LegacySeriesPersonReply,
  LegacySeriesProtocol,
  LegacySeriesReasoningBlock,
  LegacySeriesRubric,
  LegacySeriesRubricCriterion,
  LegacySeriesScoreSummary,
  LegacySeriesStageTrace,
  LegacySeriesTrial,
  LegacySeriesTrialScore,
} from './legacy-series-types';
import {
  answerTextMax,
  attemptsMax,
  booleanOf,
  choiceOf,
  idMax,
  intervalOf,
  intervalOrNull,
  invalid,
  modelTextMax,
  maxResultBytes,
  nonBlankText,
  numberAtLeast,
  recordFields,
  reasoningMax,
  replyMessagesMax,
  replyTotalMax,
  safeIntegerAtLeast,
  taskMax,
  textList,
  textOf,
  timestampOf,
} from './legacy-series-validation';

const componentIds = ['stalker', 'person-fabric', 'person', 'brain-analyzator'] as const;
const participantIds = ['a', 'b', 'neutral'] as const;
const kebabIdPattern = /^[a-z0-9]+(?:-[a-z0-9]+)*$/u;

export function decodeRubric(value: unknown): LegacySeriesRubric {
  const source = recordFields(value, ['version', 'criteria'], [], 'result.protocol.rubric');
  if (source.version !== 2) invalid("result.protocol.rubric 'version' must be 2");
  if (!Array.isArray(source.criteria) || source.criteria.length < 1 || source.criteria.length > 12)
    invalid('result.protocol.rubric.criteria must list 1 to 12 criteria');
  const seen = new Set<string>();
  const criteria = (source.criteria as unknown[]).map(
    (item, index): LegacySeriesRubricCriterion => {
      const contextPath = `result.protocol.rubric.criteria[${index}]`;
      const criterion = recordFields(
        item,
        ['id', 'label', 'weight', 'required', 'anchors'],
        [],
        contextPath,
      );
      const id = nonBlankText(criterion.id, idMax, `${contextPath}.id`);
      if (!kebabIdPattern.test(id)) invalid(`${contextPath}.id must be a kebab-case id`);
      if (seen.has(id)) invalid('result.protocol.rubric.criterion ids must be unique');
      seen.add(id);
      const anchors = recordFields(
        criterion.anchors,
        ['unmet', 'partial', 'met'],
        [],
        `${contextPath}.anchors`,
      );
      const weight = criterion.weight;
      if (typeof weight !== 'number' || !Number.isFinite(weight) || weight <= 0)
        invalid(`${contextPath}.weight must be a positive finite number`);
      return {
        id,
        label: nonBlankText(criterion.label, modelTextMax, `${contextPath}.label`),
        weight,
        required: booleanOf(criterion.required, `${contextPath}.required`),
        anchors: {
          unmet: nonBlankText(anchors.unmet, modelTextMax, `${contextPath}.anchors.unmet`),
          partial: nonBlankText(anchors.partial, modelTextMax, `${contextPath}.anchors.partial`),
          met: nonBlankText(anchors.met, modelTextMax, `${contextPath}.anchors.met`),
        },
      };
    },
  );
  return { version: 2, criteria };
}

function decodeModelSetting(value: unknown, component: string): LegacySeriesModelSetting {
  const contextPath = `result.protocol.model-settings.${component}`;
  const source = recordFields(
    value,
    ['provider', 'requested-model', 'requested-effort'],
    [],
    contextPath,
  );
  return {
    provider: choiceOf(source.provider, ['codex', 'kimi'] as const, `${contextPath}.provider`),
    requestedModel: textOf(source['requested-model'], idMax, `${contextPath}.requested-model`),
    requestedEffort: choiceOf(
      source['requested-effort'],
      ['low', 'medium', 'high'] as const,
      `${contextPath} 'requested-effort'`,
    ),
  };
}

function decodeParticipantOrder(
  value: unknown,
  repetitions: number,
): readonly (readonly string[])[] {
  if (!Array.isArray(value) || value.length !== repetitions)
    invalid("protocol 'participant-order' must list one row per repetition");
  return value.map((row, index) => {
    if (
      !Array.isArray(row) ||
      row.length !== participantIds.length ||
      (row as unknown[])
        .map((item) => nonBlankText(item, idMax, 'participant id'))
        .sort()
        .join(',') !== [...participantIds].sort().join(',')
    )
      invalid(`protocol 'participant-order' row ${index} must be a permutation of a, b, neutral`);
    return row.map((item, offset) =>
      nonBlankText(item, idMax, `protocol 'participant-order' row ${index}[${offset}]`),
    );
  });
}

export function decodeProtocol(
  value: unknown,
  context: Pick<LegacySeriesDecodeContext, 'schemaVersion'>,
): LegacySeriesProtocol {
  const optional = context.schemaVersion === 3 ? ['source-snapshots'] : [];
  const source = recordFields(
    value,
    [
      'version',
      'task',
      'rubric',
      'repetitions',
      'participant-order',
      'model-settings',
      'format-attempts',
      'stage-timeout-ms',
      'prompt-version',
      'created-at',
    ],
    optional,
    'result.protocol',
  );
  if (source.version !== 2) invalid("protocol 'version' must be 2");
  const repetitions = source.repetitions;
  if (repetitions !== 1 && repetitions !== 3) invalid("protocol 'repetitions' must be 1 or 3");
  if (
    !source['model-settings'] ||
    typeof source['model-settings'] !== 'object' ||
    Array.isArray(source['model-settings'])
  )
    invalid("protocol 'model-settings' must list exactly the four experiment components");
  const settingsSource = source['model-settings'] as Record<string, unknown>;
  if (
    Object.keys(settingsSource).length !== componentIds.length ||
    componentIds.some(
      (component) => !Object.prototype.hasOwnProperty.call(settingsSource, component),
    )
  )
    invalid(
      "protocol 'model-settings' must list exactly the stalker, person-fabric, person and brain-analyzator components",
    );
  const promptVersions =
    context.schemaVersion === 3
      ? (['bsc-v2.2-source-style', 'bsc-v2.3-source-identity'] as const)
      : (['bsc-v2.1'] as const);
  const protocol: LegacySeriesProtocol = {
    version: 2,
    task: textOf(source.task, taskMax, 'result.protocol.task'),
    rubric: decodeRubric(source.rubric),
    repetitions,
    participantOrder: decodeParticipantOrder(source['participant-order'], repetitions),
    // The exact four stable component IDs were checked above; every value is decoded.
    modelSettings: Object.fromEntries<LegacySeriesModelSetting>(
      componentIds.map((id) => [id, decodeModelSetting(settingsSource[id], id)]),
    ) as LegacySeriesProtocol['modelSettings'],
    formatAttempts: decodeLiteral2(
      source['format-attempts'],
      "protocol 'format-attempts' must be 2",
    ),
    stageTimeoutMs: decodeLiteral600000(
      source['stage-timeout-ms'],
      "protocol 'stage-timeout-ms' must be 600000",
    ),
    promptVersion: choiceOf(source['prompt-version'], promptVersions, "protocol 'prompt-version'"),
    createdAt: timestampOf(source['created-at'], "protocol 'created-at'"),
  };
  if (
    context.schemaVersion === 3 &&
    Object.prototype.hasOwnProperty.call(source, 'source-snapshots')
  ) {
    const snapshots = source['source-snapshots'];
    if (!Array.isArray(snapshots) || snapshots.length !== 2)
      invalid("protocol 'source-snapshots' must list the two participants' snapshot rows");
    Object.assign(protocol, {
      sourceSnapshots: snapshots.map((row, index) => {
        if (!Array.isArray(row) || row.length > 5)
          invalid(`protocol 'source-snapshots' row ${index} must list at most 5 snapshot ids`);
        return row.map((item, offset) =>
          nonBlankText(item, idMax, `protocol 'source-snapshots' row ${index}[${offset}]`),
        );
      }),
    });
  }
  return protocol;
}

function decodeLiteral2(value: unknown, message: string): 2 {
  if (value !== 2) invalid(message);
  return 2;
}

function decodeLiteral600000(value: unknown, message: string): 600000 {
  if (value !== 600000) invalid(message);
  return 600000;
}

function decodeReasoningBlock(value: unknown, contextPath: string): LegacySeriesReasoningBlock {
  const source = recordFields(value, ['id', 'kind', 'text'], [], contextPath);
  return {
    id: nonBlankText(source.id, maxResultBytes, `${contextPath}.id`),
    kind: choiceOf(source.kind, ['summary', 'thinking'] as const, `${contextPath}.kind`),
    text: textOf(source.text, maxResultBytes, `${contextPath}.text`),
  };
}

export function decodeIntelligenceTrace(
  value: unknown,
  contextPath: string,
): LegacySeriesIntelligenceTrace {
  const source = recordFields(
    value,
    [
      'provider',
      'reasoning-ms',
      'wall-time-ms',
      'terminal-ms',
      'summary',
      'reasoning',
      'requested-model',
      'requested-effort',
      'resolved-model',
      'applied-effort',
      'cli-version',
      'active-reasoning-ms',
      'reasoning-truncated',
    ],
    [],
    contextPath,
  );
  if (!Array.isArray(source.reasoning) || source.reasoning.length > reasoningMax)
    invalid(`${contextPath}.reasoning must be a list of at most ${reasoningMax} blocks`);
  if (source['active-reasoning-ms'] !== null)
    invalid(`${contextPath}.active-reasoning-ms must be null`);
  return {
    provider: choiceOf(source.provider, ['codex', 'kimi'] as const, `${contextPath}.provider`),
    reasoningMs: numberAtLeast(source['reasoning-ms'], 0, `${contextPath}.reasoning-ms`),
    wallTimeMs: numberAtLeast(source['wall-time-ms'], 0, `${contextPath}.wall-time-ms`),
    terminalMs: numberAtLeast(source['terminal-ms'], 0, `${contextPath}.terminal-ms`),
    summary: nonBlankText(source.summary, modelTextMax, `${contextPath}.summary`),
    reasoning: (source.reasoning as unknown[]).map((item, index) =>
      decodeReasoningBlock(item, `${contextPath}.reasoning[${index}]`),
    ),
    requestedModel: textOf(source['requested-model'], idMax, `${contextPath}.requested-model`),
    requestedEffort: choiceOf(
      source['requested-effort'],
      ['low', 'medium', 'high'] as const,
      `${contextPath} 'requested-effort'`,
    ),
    resolvedModel:
      source['resolved-model'] === null
        ? null
        : textOf(source['resolved-model'], idMax, `${contextPath}.resolved-model`),
    appliedEffort:
      source['applied-effort'] === null
        ? null
        : choiceOf(
            source['applied-effort'],
            ['low', 'medium', 'high'] as const,
            `${contextPath}.applied-effort`,
          ),
    cliVersion:
      source['cli-version'] === null
        ? null
        : textOf(source['cli-version'], idMax, `${contextPath}.cli-version`),
    activeReasoningMs: null,
    reasoningTruncated: booleanOf(
      source['reasoning-truncated'],
      `${contextPath}.reasoning-truncated`,
    ),
  };
}

function decodeInvocationAttempt(
  value: unknown,
  contextPath: string,
): LegacySeriesInvocationAttempt {
  const source = recordFields(value, ['index', 'status', 'wall-time-ms', 'error'], [], contextPath);
  const status = choiceOf(
    source.status,
    ['returned', 'format-rejected', 'accepted', 'failed'] as const,
    `${contextPath}.status`,
  );
  let error: string | null = null;
  if (status === 'failed') {
    error = textOf(source.error, modelTextMax, `${contextPath}.error`);
  } else if (source.error !== null) {
    invalid(`${contextPath}.error must be null for a '${status}' attempt`);
  }
  return {
    index: safeIntegerAtLeast(source.index, 1, `${contextPath}.index`),
    status,
    wallTimeMs: numberAtLeast(source['wall-time-ms'], 0, `${contextPath}.wall-time-ms`),
    error,
  };
}

function decodePersonReply(value: unknown, contextPath: string): LegacySeriesPersonReply {
  const source = recordFields(value, ['version', 'messages', 'style-conflicts'], [], contextPath);
  if (source.version !== 1) invalid(`'reply.version' must be 1 at ${contextPath}`);
  const messages = source.messages;
  if (
    !Array.isArray(messages) ||
    messages.length < 1 ||
    messages.length > replyMessagesMax ||
    messages.some((item) => typeof item !== 'string' || !item.trim())
  )
    invalid(`'reply.messages' must list 1 to ${replyMessagesMax} non-blank reply messages`);
  const decoded = (messages as string[]).map((item) =>
    nonBlankText(item, replyTotalMax, `${contextPath}.messages`),
  );
  if (decoded.join('').length > replyTotalMax)
    invalid(`'reply.messages' must stay within ${replyTotalMax} characters in total`);
  return {
    version: 1,
    messages: decoded,
    styleConflicts: textList(source['style-conflicts'], `${contextPath}.style-conflicts`),
  };
}

export function decodeAttempt(
  value: unknown,
  context: LegacySeriesDecodeContext,
  contextPath: string,
): LegacySeriesAttempt {
  const optional = context.schemaVersion === 3 ? ['reply'] : [];
  const source = recordFields(value, ['answer', 'trace'], optional, contextPath);
  const answer = nonBlankText(source.answer, answerTextMax, `${contextPath}.answer`);
  const attempt: LegacySeriesAttempt = {
    answer,
    trace: decodeIntelligenceTrace(source.trace, `${contextPath}.trace`),
  };
  if (context.schemaVersion === 3 && 'reply' in source) {
    const reply = decodePersonReply(source.reply, `${contextPath}.reply`);
    if (reply.messages.join('\n\n') !== answer)
      invalid(`'reply' messages do not reproduce the recorded answer at ${contextPath}`);
    Object.assign(attempt, { reply });
  }
  return attempt;
}

function decodeCriterionAssessment(
  value: unknown,
  rubricCriterionIds: readonly string[],
  contextPath: string,
): LegacySeriesCriterionAssessment {
  const source = recordFields(
    value,
    ['criterion-id', 'status', 'reason', 'quote'],
    [],
    contextPath,
  );
  const criterionId = nonBlankText(source['criterion-id'], idMax, `${contextPath}.criterion-id`);
  if (!rubricCriterionIds.includes(criterionId))
    invalid(`${contextPath}.criterion-id must reference a rubric criterion`);
  return {
    criterionId,
    status: choiceOf(
      source.status,
      ['unmet', 'partial', 'met', 'unknown'] as const,
      `${contextPath} 'status'`,
    ),
    reason: nonBlankText(source.reason, modelTextMax, `${contextPath}.reason`),
    quote: textOf(source.quote, modelTextMax, `${contextPath}.quote`),
  };
}

function decodeAnswerEvaluation(
  value: unknown,
  context: LegacySeriesDecodeContext,
  contextPath: string,
): LegacySeriesAnswerEvaluation {
  const source = recordFields(value, ['person-id', 'criteria'], [], contextPath);
  const personId = nonBlankText(source['person-id'], idMax, `${contextPath}.person-id`);
  const rubricIds = context.rubricCriteria.map((criterion) => criterion.id);
  if (!Array.isArray(source.criteria) || source.criteria.length !== rubricIds.length)
    invalid(`${contextPath}.criteria must cover every rubric criterion exactly once`);
  const decoded = (source.criteria as unknown[]).map((item, index) =>
    decodeCriterionAssessment(item, rubricIds, `${contextPath}.criteria[${index}]`),
  );
  if (new Set(decoded.map((assessment) => assessment.criterionId)).size !== rubricIds.length)
    invalid(`${contextPath}.criteria must cover every rubric criterion exactly once`);
  return { personId, criteria: decoded };
}

function decodeCriterionScore(
  value: unknown,
  context: LegacySeriesDecodeContext,
  contextPath: string,
): LegacySeriesCriterionScore {
  const source = recordFields(
    value,
    ['criterion-id', 'interval', 'assessments', 'disputed'],
    [],
    contextPath,
  );
  const rubricIds = context.rubricCriteria.map((criterion) => criterion.id);
  const criterionId = nonBlankText(source['criterion-id'], idMax, `${contextPath}.criterion-id`);
  if (!rubricIds.includes(criterionId))
    invalid(`${contextPath}.criterion-id must reference a rubric criterion`);
  const interval = intervalOf(source.interval, 0, 1, `${contextPath}.interval`);
  if (!Array.isArray(source.assessments) || source.assessments.length !== 2)
    invalid(`${contextPath}.assessments must list the two evaluation passes`);
  const assessments = (source.assessments as unknown[]).map((item, index) =>
    decodeCriterionAssessment(item, rubricIds, `${contextPath}.assessments[${index}]`),
  );
  const disputed = booleanOf(source.disputed, `${contextPath}.disputed`);
  return { criterionId, interval, assessments, disputed };
}

function decodeTrialScore(
  value: unknown,
  context: LegacySeriesDecodeContext,
  contextPath: string,
): LegacySeriesTrialScore {
  const source = recordFields(
    value,
    ['version', 'person-id', 'interval', 'criteria', 'required-status', 'judge-disagreement'],
    [],
    contextPath,
  );
  if (source.version !== 2) invalid(`${contextPath} 'version' must be 2`);
  const personId = nonBlankText(source['person-id'], idMax, `${contextPath}.person-id`);
  if (!Array.isArray(source.criteria) || source.criteria.length !== context.rubricCriteria.length)
    invalid(`${contextPath}.criteria must match the rubric criterion set`);
  const rubricIds = context.rubricCriteria.map((criterion) => criterion.id);
  const criteria = (source.criteria as unknown[]).map((item, index) =>
    decodeCriterionScore(item, context, `${contextPath}.criteria[${index}]`),
  );
  if (
    new Set(criteria.map((criterion) => criterion.criterionId)).size !== rubricIds.length ||
    criteria.some((criterion) => !rubricIds.includes(criterion.criterionId))
  )
    invalid(`${contextPath}.criteria must match the rubric criterion set`);
  return {
    version: 2,
    personId,
    interval: intervalOf(source.interval, 0, 100, `${contextPath}.interval`),
    criteria,
    requiredStatus: choiceOf(
      source['required-status'],
      ['passed', 'failed', 'unknown'] as const,
      `${contextPath}.required-status`,
    ),
    judgeDisagreement: booleanOf(source['judge-disagreement'], `${contextPath}.judge-disagreement`),
  };
}

export function decodeScoreSummary(
  value: unknown,
  repetitions: 1 | 3,
  contextPath: string,
): LegacySeriesScoreSummary {
  const source = recordFields(
    value,
    [
      'version',
      'person-id',
      'interval',
      'point',
      'planned',
      'evaluated',
      'complete',
      'spread',
      'required-status',
      'judge-disagreement',
      'comparison-eligible',
    ],
    [],
    contextPath,
  );
  if (source.version !== 2) invalid(`${contextPath} 'version' must be 2`);
  const planned = safeIntegerAtLeast(source.planned, 1, `${contextPath} 'planned'`);
  if (planned !== repetitions)
    invalid(`${contextPath} 'planned' must equal protocol 'repetitions'`);
  const evaluated = safeIntegerAtLeast(source.evaluated, 0, `${contextPath} 'evaluated'`);
  if (evaluated > planned) invalid(`${contextPath} 'evaluated' must not exceed 'planned'`);
  const complete = booleanOf(source.complete, `${contextPath} 'complete'`);
  if (complete !== (evaluated === planned))
    invalid(`${contextPath} 'complete' must equal ('evaluated' === 'planned')`);
  const interval = intervalOrNull(source.interval, 0, 100, `${contextPath}.interval`);
  let point: number | null = null;
  if (source.point !== null) {
    if (typeof source.point !== 'number' || !Number.isFinite(source.point))
      invalid(`${contextPath}.point must be a finite number or null`);
    point = source.point;
    if (!interval) invalid(`${contextPath} 'point' requires a recorded interval`);
    if (!complete) invalid(`${contextPath} 'point' requires a complete summary`);
    if (point !== interval.lower || point !== interval.upper)
      invalid(`${contextPath} 'point' must equal the collapsed interval`);
  }
  const spread = intervalOrNull(source.spread, 0, 100, `${contextPath}.spread`);
  if ((interval === null) !== (evaluated === 0))
    invalid(`${contextPath}.interval must be present exactly when trials were evaluated`);
  if ((spread === null) !== (evaluated === 0))
    invalid(`${contextPath}.spread must be present exactly when trials were evaluated`);
  if (interval && spread && (interval.lower < spread.lower || interval.upper > spread.upper))
    invalid(`${contextPath}.interval must lie within 'spread'`);
  return {
    version: 2,
    personId: nonBlankText(source['person-id'], idMax, `${contextPath}.person-id`),
    interval,
    point,
    planned,
    evaluated,
    complete,
    spread,
    requiredStatus: choiceOf(
      source['required-status'],
      ['passed', 'failed', 'unknown'] as const,
      `${contextPath}.required-status`,
    ),
    judgeDisagreement: booleanOf(source['judge-disagreement'], `${contextPath}.judge-disagreement`),
    comparisonEligible: booleanOf(
      source['comparison-eligible'],
      `${contextPath}.comparison-eligible`,
    ),
  };
}

export function decodeTrial(
  value: unknown,
  context: LegacySeriesDecodeContext,
  contextPath: string,
): LegacySeriesTrial {
  const source = recordFields(
    value,
    ['id', 'round', 'person-id', 'status', 'attempt', 'score', 'evaluations', 'error'],
    [],
    contextPath,
  );
  const id = nonBlankText(source.id, idMax, `${contextPath}.id`);
  const round = safeIntegerAtLeast(source.round, 1, `${contextPath}.round`);
  const personId = nonBlankText(source['person-id'], idMax, `${contextPath}.person-id`);
  const status = choiceOf(
    source.status,
    ['pending', 'answered', 'evaluated', 'failed', 'skipped'] as const,
    `${contextPath}.status`,
  );
  const attempt =
    source.attempt === null
      ? null
      : decodeAttempt(source.attempt, context, `${contextPath}.attempt`);
  const score =
    source.score === null ? null : decodeTrialScore(source.score, context, `${contextPath}.score`);
  if (!Array.isArray(source.evaluations) || source.evaluations.length > 2)
    invalid(`${contextPath}.evaluations must list at most two evaluation passes`);
  const evaluations = (source.evaluations as unknown[]).map((item, index) =>
    decodeAnswerEvaluation(item, context, `${contextPath}.evaluations[${index}]`),
  );
  const error =
    source.error === null ? null : textOf(source.error, modelTextMax, `${contextPath}.error`);
  const trial = { id, round, personId, status, attempt, score, evaluations, error };
  validateTrialState(trial);
  return trial;
}
function validateTrialState(trial: LegacySeriesTrial): void {
  const { id, status, attempt, score, evaluations, error } = trial;
  if (status === 'evaluated') {
    if (!score) invalid(`trial '${id}' is an evaluated trial without a recorded score`);
    if (!attempt) invalid(`trial '${id}' is an evaluated trial without a recorded attempt`);
    if (evaluations.length !== 2)
      invalid(`trial '${id}' is an evaluated trial without two evaluation passes`);
    if (error !== null) invalid(`trial '${id}' is an evaluated trial carrying an error`);
  } else if (status === 'answered') {
    if (score || !attempt) invalid(`an 'answered' trial requires an attempt and no score`);
  } else if (status === 'skipped') {
    if (attempt || score || evaluations.length)
      invalid('a skipped trial must not carry an attempt, a score or evaluations');
    if (error === null) invalid(`a skipped trial requires the failure message in 'error'`);
  } else if (status === 'failed') {
    if (attempt || score) invalid(`a 'failed' trial must not carry an attempt or score`);
    if (evaluations.length) invalid(`a 'failed' trial must not carry evaluations`);
    if (error === null) invalid(`a 'failed' trial requires the failure message in 'error'`);
  } else if (status === 'pending') {
    if (attempt || score || evaluations.length || error !== null)
      invalid(`a 'pending' trial must be empty`);
  }
}

export function decodeStageTrace(value: unknown, contextPath: string): LegacySeriesStageTrace {
  const source = recordFields(
    value,
    ['id', 'stage', 'person', 'label', 'trace', 'attempts'],
    [],
    contextPath,
  );
  const id = nonBlankText(source.id, idMax, `${contextPath}.id`);
  const person = source.person;
  if (
    person !== null &&
    (typeof person !== 'number' || !Number.isInteger(person) || person < 0 || person > 2)
  )
    invalid(`${contextPath}.person must be null or a participant index`);
  if (!Array.isArray(source.attempts) || source.attempts.length > attemptsMax)
    invalid(`${contextPath}.attempts must list at most ${attemptsMax} invocation attempts`);
  const attempts = (source.attempts as unknown[]).map((item, index) => {
    const decoded = decodeInvocationAttempt(item, `${contextPath}.attempts[${index}]`);
    if (decoded.index !== index + 1)
      invalid(`${contextPath}.attempts must be indexed from 1 in invocation order`);
    return decoded;
  });
  return {
    id,
    stage: choiceOf(
      source.stage,
      ['stalker', 'person-fabric', 'person', 'brain-analyzator'] as const,
      `${contextPath}.stage`,
    ),
    person,
    label: nonBlankText(source.label, modelTextMax, `${contextPath}.label`),
    trace: decodeIntelligenceTrace(source.trace, `${contextPath}.trace`),
    attempts,
  };
}
