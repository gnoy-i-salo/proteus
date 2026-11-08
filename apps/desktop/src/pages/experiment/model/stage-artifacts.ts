import type {
  AnswerEvaluation,
  EvaluationRubric,
  IntelligenceOperation,
  InvocationAttempt,
  ModelCallLifecycle,
  PersonProfile,
  PersonReply,
  RetrievalReceipt,
  StageLifecycle,
  TrialScore,
} from '@proteus-int/contracts';
import type {
  ExperimentPerson,
  ExperimentResult,
  StageTrace,
  TrialResult,
} from '@proteus-int/pipeline';
import { classifyExecutionStage } from './execution-stage';
import type { ExecutionStageClassification } from './execution-stage';

/** Persisted research report of a participant; derived from the result shape, not redeclared. */
type PersistedReport = NonNullable<ExperimentPerson['report']>;

/** Known persisted stage identities; the id grammar is owned by the pipeline producers. */
export type StageArtifactRequest =
  | { readonly kind: 'rubric' }
  | { readonly kind: 'research'; readonly person: 0 | 1 }
  | { readonly kind: 'profile'; readonly person: 0 | 1 }
  | { readonly kind: 'adaptation'; readonly participant: 0 | 1 | 2 }
  | {
      readonly kind: 'solution';
      readonly round: 1 | 2 | 3;
      readonly participant: 'a' | 'b' | 'neutral';
    }
  | { readonly kind: 'judgment'; readonly round: 1 | 2 | 3; readonly pass: 0 | 1 };

/** Explicit availability slot: unavailable states always carry a machine-readable reason. */
export type StageSlot<T, Reason extends string> =
  | { readonly status: 'available'; readonly value: T }
  | { readonly status: 'unavailable'; readonly reason: Reason };

/** Trace join for a stage id: duplicates and identity contradictions stay explicit. */
export type StageTraceLocation = StageSlot<
  StageTrace,
  'trace-not-found' | 'duplicate-stage-traces' | 'stage-identity-mismatch'
>;

/** Lifecycle join for a stage id; the durable record stays unique or explicitly unavailable. */
export type StageLifecycleLocation = StageSlot<
  StageLifecycle,
  'lifecycle-not-found' | 'duplicate-stage-lifecycles' | 'stage-identity-mismatch'
>;

/** Located persisted observations for a stage; every absence or ambiguity stays explicit. */
export interface StageLocation {
  /** The result carries the execution extension (false for historical schema4 results). */
  readonly executionPresent: boolean;
  /** The single trace persisted under the stage id; never chosen among duplicates. */
  readonly trace: StageTraceLocation;
  /** Durable lifecycle record (execution snapshot, else the consistent trace-embedded one). */
  readonly lifecycle: StageLifecycleLocation;
}

/** One model call joined positionally with its invocation attempt; nothing is inferred. */
export interface StageCall {
  /** 1-based position shared by lifecycle calls and trace attempts. */
  readonly index: number;
  readonly callId: string;
  readonly operation: IntelligenceOperation;
  readonly trigger: 'initial' | 'formatRepair';
  readonly formatAttempt: 1 | 2;
  readonly status: ModelCallLifecycle['status'];
  /** Attempt status at the same position; null when the attempt list is shorter. */
  readonly attemptStatus: InvocationAttempt['status'] | null;
  /** Per-call duration from the lifecycle; never the cumulative trace duration; null if unmeasured. */
  readonly wallTimeMs: number | null;
  readonly error: string | null;
  readonly validationError: string | null;
}

export type StageCalls =
  | { readonly status: 'available'; readonly calls: readonly StageCall[] }
  | {
      readonly status: 'unavailable';
      readonly reason:
        | 'legacy-execution-metadata-missing'
        | 'stage-not-recorded'
        | 'duplicate-stage-traces'
        | 'duplicate-stage-lifecycles'
        | 'stage-identity-mismatch'
        | 'attempt-index-mismatch'
        | 'duplicate-call-ids';
    };

/**
 * Recall receipts are typed against the real contract, but no persisted producer emits them yet;
 * the slot stays explicitly unavailable until the rights-gated inspector attaches real receipts.
 */
export type StageReceipts = StageSlot<readonly RetrievalReceipt[], 'recall-receipts-not-persisted'>;

export interface RubricArtifact {
  /** Exact common task text, referenced from the persisted protocol. */
  readonly task: string;
  /** Rubric frozen before answers, referenced from the persisted protocol. */
  readonly rubric: EvaluationRubric;
}

export interface ResearchArtifact {
  readonly report: PersistedReport;
  /** Immutable corpus snapshot ids granted for this person at protocol time. */
  readonly sourceSnapshotIds: readonly string[];
}

export interface SolutionArtifact {
  readonly trial: TrialResult;
  readonly answer: StageSlot<string, 'answer-not-produced' | 'trial-failed' | 'trial-skipped'>;
  /** Persisted structured reply only; never synthesized from the answer text. */
  readonly reply: StageSlot<PersonReply, 'reply-not-persisted'>;
  readonly score: StageSlot<TrialScore, 'not-scored'>;
}

export interface JudgmentPersonEntry {
  readonly personId: string;
  /** null when this person was not assessed in the requested pass (partial series). */
  readonly evaluation: AnswerEvaluation | null;
}

export interface JudgmentArtifact {
  readonly round: number;
  /** 0 is the stored forward pass, 1 the stored reverse pass, by producer append order. */
  readonly pass: 0 | 1;
  readonly assessments: readonly JudgmentPersonEntry[];
}

interface StageArtifactBase {
  /** Exact persisted stage id; traces and lifecycle join on it. */
  readonly stageId: string;
  /** Shared execution-stage identity; derived from located data, never from labels. */
  readonly classification: ExecutionStageClassification;
  readonly stage: StageLocation;
  readonly calls: StageCalls;
  readonly receipts: StageReceipts;
}

export type StageArtifact =
  | (StageArtifactBase & {
      readonly request: { readonly kind: 'rubric' };
      readonly rubric: RubricArtifact;
    })
  | (StageArtifactBase & {
      readonly request: { readonly kind: 'research' };
      readonly report: StageSlot<ResearchArtifact, 'artifact-not-produced'>;
    })
  | (StageArtifactBase & {
      readonly request: { readonly kind: 'profile' };
      /** Task-free base profile produced by the profile stage. */
      readonly profile: StageSlot<PersonProfile, 'artifact-not-produced'>;
    })
  | (StageArtifactBase & {
      readonly request: { readonly kind: 'adaptation' };
      /** Adapted profile produced by the adaptation stage (person or neutral control). */
      readonly profile: StageSlot<PersonProfile, 'artifact-not-produced'>;
    })
  | (StageArtifactBase & {
      readonly request: {
        readonly kind: 'solution';
        readonly round: 1 | 2 | 3;
        readonly participant: 'a' | 'b' | 'neutral';
      };
      readonly solution: StageSlot<SolutionArtifact, 'trial-not-planned' | 'duplicate-trials'>;
    })
  | (StageArtifactBase & {
      readonly request: {
        readonly kind: 'judgment';
        readonly round: 1 | 2 | 3;
        readonly pass: 0 | 1;
      };
      readonly judgment: StageSlot<
        JudgmentArtifact,
        'evaluations-not-produced' | 'duplicate-trials' | 'evaluation-person-mismatch'
      >;
    });

const participantIds = ['a', 'b', 'neutral'] as const;

const plannedRounds = (repetitions: 1 | 3): readonly (1 | 2 | 3)[] =>
  repetitions === 1 ? [1] : [1, 2, 3];

/**
 * No persisted producer emits RetrievalReceipt data inside results today; the seam stays
 * explicitly unavailable until the rights-gated inspector attaches real receipts (tip.15).
 */
const unavailableReceipts: StageReceipts = {
  status: 'unavailable',
  reason: 'recall-receipts-not-persisted',
};

/** Projects the request to the producer identity used when no stage data was located. */
function projectedIdentity(request: StageArtifactRequest): {
  readonly logicalStageId: string;
  readonly component: string;
  readonly person: number | null;
  readonly label: string;
} {
  return {
    logicalStageId: stageArtifactStageId(request),
    component:
      request.kind === 'rubric' || request.kind === 'judgment'
        ? 'brain-analyzator'
        : request.kind === 'research'
          ? 'stalker'
          : request.kind === 'solution'
            ? 'person'
            : 'person-fabric',
    person:
      request.kind === 'research' || request.kind === 'profile'
        ? request.person
        : request.kind === 'adaptation'
          ? request.participant
          : request.kind === 'solution'
            ? participantIds.indexOf(request.participant)
            : null,
    label: '',
  };
}

/** Persisted identity fields a trace and a lifecycle must agree on to be one stage's observations. */
interface PersistedStageIdentity {
  readonly logicalStageId: string;
  readonly component: string;
  readonly person: number | null;
}

function traceIdentity(trace: StageTrace): PersistedStageIdentity {
  return { logicalStageId: trace.id, component: trace.stage, person: trace.person };
}

function lifecycleIdentity(lifecycle: StageLifecycle): PersistedStageIdentity {
  return {
    logicalStageId: lifecycle.logicalStageId,
    component: lifecycle.component,
    person: lifecycle.person,
  };
}

function sameIdentity(first: PersistedStageIdentity, second: PersistedStageIdentity): boolean {
  return (
    first.logicalStageId === second.logicalStageId &&
    first.component === second.component &&
    first.person === second.person
  );
}

function identitiesConsistent(identities: readonly PersistedStageIdentity[]): boolean {
  return (
    identities.length < 2 || identities.every((identity) => sameIdentity(identity, identities[0]!))
  );
}

function locateStage(
  result: Pick<ExperimentResult, 'traces' | 'execution'>,
  stageId: string,
): StageLocation {
  const executionPresent = result.execution !== undefined;
  const traceMatches = result.traces.filter((item) => item.id === stageId);
  const lifecycleMatches =
    result.execution?.stages.filter((stage) => stage.logicalStageId === stageId) ?? [];
  const trace = traceMatches.length === 1 ? traceMatches[0]! : null;
  const executionLifecycle = lifecycleMatches.length === 1 ? lifecycleMatches[0]! : null;
  if (traceMatches.length > 1) {
    return {
      executionPresent,
      trace: { status: 'unavailable', reason: 'duplicate-stage-traces' },
      lifecycle:
        lifecycleMatches.length > 1
          ? { status: 'unavailable', reason: 'duplicate-stage-lifecycles' }
          : executionLifecycle
            ? { status: 'available', value: executionLifecycle }
            : { status: 'unavailable', reason: 'lifecycle-not-found' },
    };
  }
  if (lifecycleMatches.length > 1) {
    return {
      executionPresent,
      trace: trace
        ? trace.lifecycle && !sameIdentity(traceIdentity(trace), lifecycleIdentity(trace.lifecycle))
          ? { status: 'unavailable', reason: 'stage-identity-mismatch' }
          : { status: 'available', value: trace }
        : { status: 'unavailable', reason: 'trace-not-found' },
      lifecycle: { status: 'unavailable', reason: 'duplicate-stage-lifecycles' },
    };
  }
  const identities: PersistedStageIdentity[] = [];
  if (executionLifecycle) identities.push(lifecycleIdentity(executionLifecycle));
  if (trace) {
    identities.push(traceIdentity(trace));
    if (trace.lifecycle) identities.push(lifecycleIdentity(trace.lifecycle));
  }
  const embedded = trace?.lifecycle;
  const inconsistentRun =
    executionLifecycle &&
    embedded &&
    (executionLifecycle.runId !== embedded.runId ||
      executionLifecycle.stageId !== embedded.stageId);
  if (!identitiesConsistent(identities) || inconsistentRun) {
    return {
      executionPresent,
      trace: { status: 'unavailable', reason: 'stage-identity-mismatch' },
      lifecycle: { status: 'unavailable', reason: 'stage-identity-mismatch' },
    };
  }
  return {
    executionPresent,
    trace: trace
      ? { status: 'available', value: trace }
      : { status: 'unavailable', reason: 'trace-not-found' },
    lifecycle: executionLifecycle
      ? { status: 'available', value: executionLifecycle }
      : trace?.lifecycle
        ? { status: 'available', value: trace.lifecycle }
        : { status: 'unavailable', reason: 'lifecycle-not-found' },
  };
}

function projectCalls(stage: StageLocation): StageCalls {
  const lifecycle = stage.lifecycle;
  if (lifecycle.status !== 'available') {
    if (lifecycle.reason === 'lifecycle-not-found') {
      return {
        status: 'unavailable',
        reason: stage.executionPresent ? 'stage-not-recorded' : 'legacy-execution-metadata-missing',
      };
    }
    return { status: 'unavailable', reason: lifecycle.reason };
  }
  if (
    new Set(lifecycle.value.calls.map((call) => call.callId)).size !== lifecycle.value.calls.length
  ) {
    return { status: 'unavailable', reason: 'duplicate-call-ids' };
  }
  if (stage.trace.status === 'unavailable') {
    if (stage.trace.reason === 'trace-not-found') {
      return { status: 'available', calls: joinAttempts(lifecycle.value.calls, []) };
    }
    return { status: 'unavailable', reason: stage.trace.reason };
  }
  const attempts = stage.trace.value.attempts;
  const calls = lifecycle.value.calls;
  const waitingForAttempt =
    attempts.length === calls.length - 1 && calls.at(-1)?.status === 'running';
  if (
    (attempts.length !== calls.length && !waitingForAttempt) ||
    !attempts.every((attempt, position) => attempt.index === position + 1)
  ) {
    return { status: 'unavailable', reason: 'attempt-index-mismatch' };
  }
  return { status: 'available', calls: joinAttempts(calls, attempts) };
}

function joinAttempts(
  calls: readonly ModelCallLifecycle[],
  attempts: readonly InvocationAttempt[],
): readonly StageCall[] {
  return calls.map((call, position) => {
    const attempt = attempts[position];
    return {
      index: position + 1,
      callId: call.callId,
      operation: call.operation,
      trigger: call.trigger,
      formatAttempt: call.formatAttempt,
      status: call.status,
      attemptStatus: attempt?.status ?? null,
      wallTimeMs: call.wallTimeMs,
      error: call.error,
      validationError: call.validationError,
    };
  });
}

function classifyStage(
  request: StageArtifactRequest,
  stage: StageLocation,
): ExecutionStageClassification {
  if (stage.lifecycle.status === 'available') {
    return classifyExecutionStage({
      logicalStageId: stage.lifecycle.value.logicalStageId,
      component: stage.lifecycle.value.component,
      person: stage.lifecycle.value.person,
      label: stage.lifecycle.value.label,
    });
  }
  if (stage.trace.status === 'available') {
    return classifyExecutionStage({
      logicalStageId: stage.trace.value.id,
      component: stage.trace.value.stage,
      person: stage.trace.value.person,
      label: stage.trace.value.label,
    });
  }
  return classifyExecutionStage(projectedIdentity(request));
}

function researchArtifact(
  result: ExperimentResult,
  person: 0 | 1,
): StageSlot<ResearchArtifact, 'artifact-not-produced'> {
  const report = result.people[person]?.report ?? null;
  if (!report) return { status: 'unavailable', reason: 'artifact-not-produced' };
  return {
    status: 'available',
    value: { report, sourceSnapshotIds: result.protocol.sourceSnapshots?.[person] ?? [] },
  };
}

function profileSlot(
  person: ExperimentPerson | undefined,
  pick: (value: ExperimentPerson) => PersonProfile,
): StageSlot<PersonProfile, 'artifact-not-produced'> {
  if (!person) return { status: 'unavailable', reason: 'artifact-not-produced' };
  return { status: 'available', value: pick(person) };
}

function solutionSlot(
  result: ExperimentResult,
  round: 1 | 2 | 3,
  participant: 'a' | 'b' | 'neutral',
): StageSlot<SolutionArtifact, 'trial-not-planned' | 'duplicate-trials'> {
  const matches = result.trials.filter(
    (item) => item.round === round && item.personId === participant,
  );
  if (matches.length === 0) return { status: 'unavailable', reason: 'trial-not-planned' };
  if (matches.length > 1) return { status: 'unavailable', reason: 'duplicate-trials' };
  const trial = matches[0]!;
  const attempt = trial.attempt;
  const answer: SolutionArtifact['answer'] = !attempt
    ? {
        status: 'unavailable',
        reason:
          trial.status === 'failed'
            ? 'trial-failed'
            : trial.status === 'skipped'
              ? 'trial-skipped'
              : 'answer-not-produced',
      }
    : { status: 'available', value: attempt.answer };
  const reply: SolutionArtifact['reply'] = attempt?.reply
    ? { status: 'available', value: attempt.reply }
    : { status: 'unavailable', reason: 'reply-not-persisted' };
  const score: SolutionArtifact['score'] = trial.score
    ? { status: 'available', value: trial.score }
    : { status: 'unavailable', reason: 'not-scored' };
  return { status: 'available', value: { trial, answer, reply, score } };
}

function judgmentSlot(
  result: ExperimentResult,
  round: 1 | 2 | 3,
  pass: 0 | 1,
): StageSlot<
  JudgmentArtifact,
  'evaluations-not-produced' | 'duplicate-trials' | 'evaluation-person-mismatch'
> {
  const roundTrials = result.trials.filter((trial) => trial.round === round);
  const byPerson = new Map<string, number>();
  for (const trial of roundTrials) {
    byPerson.set(trial.personId, (byPerson.get(trial.personId) ?? 0) + 1);
  }
  if ([...byPerson.values()].some((count) => count > 1)) {
    return { status: 'unavailable', reason: 'duplicate-trials' };
  }
  const assessments: JudgmentPersonEntry[] = roundTrials.map((trial) => ({
    personId: trial.personId,
    evaluation: trial.evaluations[pass] ?? null,
  }));
  if (
    assessments.some((entry) => entry.evaluation && entry.evaluation.personId !== entry.personId)
  ) {
    return { status: 'unavailable', reason: 'evaluation-person-mismatch' };
  }
  if (!assessments.some((entry) => entry.evaluation)) {
    return { status: 'unavailable', reason: 'evaluations-not-produced' };
  }
  return { status: 'available', value: { round, pass, assessments } };
}

/** Projects a known stage request to the exact persisted stage id (never parsed from labels). */
export function stageArtifactStageId(request: StageArtifactRequest): string {
  switch (request.kind) {
    case 'rubric':
      return 'rubric';
    case 'research':
      return `sources-${request.person}`;
    case 'profile':
      return `profile-${request.person}`;
    case 'adaptation':
      return `adapt-${request.participant}`;
    case 'solution':
      return `solve-${request.round}-${request.participant}`;
    case 'judgment':
      return `judge-${request.round}-${request.pass}`;
  }
}

/**
 * Enumerates the planned stage requests for a persisted result, in pipeline order.
 * Per-round solution order follows the persisted protocol.participantOrder rotation
 * (the same order runTrials stores trials in); absent entries fall back to the
 * canonical participant list, and entries that are not known participant ids are skipped.
 */
export function stageArtifactRequests(result: ExperimentResult): StageArtifactRequest[] {
  const requests: StageArtifactRequest[] = [{ kind: 'rubric' }];
  for (const person of [0, 1] as const) {
    requests.push({ kind: 'research', person }, { kind: 'profile', person });
  }
  for (const participant of [0, 1, 2] as const) {
    requests.push({ kind: 'adaptation', participant });
  }
  for (const round of plannedRounds(result.protocol.repetitions)) {
    const planned = result.protocol.participantOrder[round - 1] ?? participantIds;
    for (const participant of planned) {
      if (participant === 'a' || participant === 'b' || participant === 'neutral') {
        requests.push({ kind: 'solution', round, participant });
      }
    }
    requests.push({ kind: 'judgment', round, pass: 0 }, { kind: 'judgment', round, pass: 1 });
  }
  return requests;
}

/** Pure projection of the persisted artifacts for one known stage identity. */
export function projectStageArtifact(
  result: ExperimentResult,
  request: StageArtifactRequest,
): StageArtifact {
  const stageId = stageArtifactStageId(request);
  const stage = locateStage(result, stageId);
  const base = {
    stageId,
    classification: classifyStage(request, stage),
    stage,
    calls: projectCalls(stage),
    receipts: unavailableReceipts,
  };
  switch (request.kind) {
    case 'rubric':
      return {
        ...base,
        request,
        rubric: { task: result.protocol.task, rubric: result.protocol.rubric },
      };
    case 'research':
      return { ...base, request, report: researchArtifact(result, request.person) };
    case 'profile':
      return {
        ...base,
        request,
        profile: profileSlot(result.people[request.person], (person) => person.baseProfile),
      };
    case 'adaptation':
      return {
        ...base,
        request,
        profile:
          request.participant === 2
            ? profileSlot(result.control, (person) => person.profile)
            : profileSlot(result.people[request.participant], (person) => person.profile),
      };
    case 'solution':
      return {
        ...base,
        request,
        solution: solutionSlot(result, request.round, request.participant),
      };
    case 'judgment':
      return {
        ...base,
        request,
        judgment: judgmentSlot(result, request.round, request.pass),
      };
  }
}

/** The same observation join for a live trace, before a final result exists. */
export function projectLiveStage(trace: StageTrace): {
  readonly stage: StageLocation;
  readonly calls: StageCalls;
} {
  const stage = locateStage({ traces: [trace] }, trace.id);
  return { stage, calls: projectCalls(stage) };
}
