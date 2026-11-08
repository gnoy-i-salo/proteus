import { describe, expect, it } from 'vitest';
import type {
  AnswerEvaluation,
  EvaluationRubric,
  IntelligenceTrace,
  ModelCallLifecycle,
  PersonProfile,
  ScoreSummary,
  StageLifecycle,
} from '@proteus-int/contracts';
import type {
  ExperimentPerson,
  ExperimentResult,
  StageTrace,
  TrialResult,
} from '@proteus-int/pipeline';
import { componentIds } from '@proteus-int/intelligence';
import {
  projectStageArtifact,
  stageArtifactRequests,
  stageArtifactStageId,
} from './stage-artifacts';
import type { StageArtifact } from './stage-artifacts';

const taskText = 'Общая задача\nс сохранёнными  пробелами и Юникодом 🧪';

function ofKind<Kind extends StageArtifact['request']['kind']>(
  artifact: StageArtifact,
  kind: Kind,
): Extract<StageArtifact, { readonly request: { readonly kind: Kind } }> {
  expect(artifact.request.kind).toBe(kind);
  return artifact as Extract<StageArtifact, { readonly request: { readonly kind: Kind } }>;
}

function rubric(): EvaluationRubric {
  return {
    version: 2,
    criteria: [
      {
        id: 'criterion-1',
        label: 'Полнота ответа',
        weight: 2,
        required: true,
        anchors: { unmet: 'Не раскрыто', partial: 'Частично', met: 'Полностью раскрыто' },
      },
      {
        id: 'criterion-2',
        label: 'Точность',
        weight: 1,
        required: false,
        anchors: { unmet: 'Ошибки', partial: 'Спорно', met: 'Без ошибок' },
      },
    ],
  };
}

function trace(partial: Partial<IntelligenceTrace> = {}): IntelligenceTrace {
  return {
    provider: 'codex',
    reasoningMs: 1200,
    wallTimeMs: 1200,
    activeReasoningMs: null,
    requestedModel: 'opus',
    resolvedModel: null,
    requestedEffort: 'high',
    appliedEffort: 'high',
    cliVersion: null,
    terminalMs: 0,
    summary: 'Запрос завершён',
    ...partial,
  };
}

function call(partial: Partial<ModelCallLifecycle> = {}): ModelCallLifecycle {
  return {
    callId: 'call-1',
    operation: 'generation',
    trigger: 'initial',
    formatAttempt: 1,
    status: 'completed',
    startedAt: '2026-11-08T10:00:00.000Z',
    returnedAt: '2026-11-08T10:00:01.000Z',
    finishedAt: '2026-11-08T10:00:01.000Z',
    wallTimeMs: 1000,
    error: null,
    validationError: null,
    ...partial,
  };
}

function lifecycle(
  logicalStageId: string,
  component: StageLifecycle['component'],
  person: number | null,
  status: StageLifecycle['status'],
  calls: readonly ModelCallLifecycle[],
  error: string | null = null,
): StageLifecycle {
  return {
    schemaVersion: 1,
    runId: 'run-1',
    stageId: `uuid-${logicalStageId}`,
    logicalStageId,
    component,
    person,
    label: `Метка не парсится: ${logicalStageId}`,
    status,
    startedAt: '2026-11-08T10:00:00.000Z',
    finishedAt: '2026-11-08T10:00:02.000Z',
    error,
    calls,
  };
}

function stageTrace(
  id: string,
  stage: StageTrace['stage'],
  person: number | null,
  partial: Partial<StageTrace> = {},
): StageTrace {
  return {
    id,
    stage,
    person,
    label: 'Произвольная метка этапа',
    trace: trace(),
    attempts: [{ index: 1, status: 'accepted', wallTimeMs: 1000, error: null }],
    ...partial,
  };
}

function profile(
  id: string,
  label: string,
  sourceStatus: PersonProfile['sourceStatus'],
): PersonProfile {
  return {
    id,
    label,
    palette: id === 'b' ? 'violet' : 'turquoise',
    archetype: `Архетип ${id}`,
    skills: [`Навык ${id}`],
    rules: [],
    taskSkills: [],
    sourceStatus,
  };
}

function evaluation(personId: string, quote: string): AnswerEvaluation {
  return {
    personId,
    criteria: [
      {
        criterionId: 'criterion-1',
        status: 'met',
        reason: 'Обоснование оценки сохранено дословно',
        quote,
      },
    ],
  };
}

function trial(round: number, personId: string, partial: Partial<TrialResult> = {}): TrialResult {
  return {
    id: `${round}-${personId}`,
    round,
    personId,
    status: 'evaluated',
    attempt: {
      answer: `Ответ ${personId} прогон ${round}`,
      trace: trace(),
      reply: {
        version: 2,
        messages: [`Ответ ${personId}`, `прогон ${round}`],
        styleConflicts: [],
      },
    },
    score: null,
    evaluations: [evaluation(personId, `цитата-${personId}-${round}-прямой`)],
    error: null,
    ...partial,
  };
}

function scoreSummary(personId: string): ScoreSummary {
  return {
    version: 2,
    personId,
    interval: null,
    point: null,
    planned: 3,
    evaluated: 1,
    complete: false,
    spread: null,
    requiredStatus: 'unknown',
    judgeDisagreement: false,
    comparisonEligible: false,
  };
}

function person(
  id: string,
  label: string,
  sourceStatus: PersonProfile['sourceStatus'],
  partial: Partial<ExperimentPerson> = {},
): ExperimentPerson {
  return {
    report: {
      evidence: [
        {
          id: 'evidence-1',
          sourceUrl: 'https://example.test/page',
          kind: 'fact',
          summary: 'Сводка свидетельства',
          status: 'supported',
          verification: 'model-reported',
          excerpt: 'Точная выдержка\u00A0с неразрывным пробелом',
          retrievedAt: null,
          limitations: [],
        },
      ],
      character: 'Характер',
      thinkingStyle: 'Стиль мышления',
      activity: 'Деятельность',
      behaviorSummary: 'Резюме поведения',
    },
    baseProfile: profile(id, `${label} (базовый)`, sourceStatus),
    profile: profile(id, `${label} (адаптированный)`, sourceStatus),
    attempt: null,
    score: scoreSummary(id),
    trials: [],
    preparationWallTimeMs: 1000,
    solutionWallTimeMs: 2000,
    baselineDifference: null,
    artifacts: {},
    ...partial,
  };
}

interface FixtureOptions {
  readonly repetitions?: 1 | 3;
  readonly traces?: readonly StageTrace[];
  readonly stages?: readonly StageLifecycle[];
  readonly trials?: readonly TrialResult[];
  readonly people?: readonly [ExperimentPerson, ExperimentPerson];
  readonly control?: ExperimentPerson;
  readonly execution?: boolean;
  /** Overrides the persisted plan order; defaults to the producer rotation grammar. */
  readonly participantOrder?: readonly (readonly string[])[];
}

/** Mirrors the producer rotation: round r starts at participant index r (buildTrialOrder). */
function rotatedOrder(repetitions: 1 | 3): readonly (readonly string[])[] {
  const ids = ['a', 'b', 'neutral'] as const;
  return Array.from({ length: repetitions }, (_, round) =>
    ids.map((_, offset) => ids[(round + offset) % ids.length]!),
  );
}

function result(options: FixtureOptions = {}): ExperimentResult {
  const repetitions = options.repetitions ?? 1;
  const first = options.people?.[0] ?? person('a', 'Персона A', 'evidence-informed');
  const second = options.people?.[1] ?? person('b', 'Персона B', 'synthetic');
  const neutral = options.control ?? person('neutral', 'Нейтральный контроль', 'neutral');
  return {
    schemaVersion: 4,
    ...(options.execution === false
      ? {}
      : {
          execution: {
            schemaVersion: 1 as const,
            runId: 'run-1',
            stages: [...(options.stages ?? [])],
          },
        }),
    scoringVersion: 2,
    status: 'complete',
    input: {
      task: taskText,
      people: [
        { label: 'A', context: 'контекст A', publicUrls: [] },
        { label: 'B', context: 'контекст B', publicUrls: [] },
      ],
    },
    protocol: {
      version: 3,
      task: taskText,
      rubric: rubric(),
      repetitions,
      participantOrder: options.participantOrder ?? rotatedOrder(repetitions),
      modelSettings: Object.fromEntries(
        componentIds.map((component) => [
          component,
          {
            provider: 'codex' as const,
            requestedModel: component === 'person' ? 'opus' : '',
            requestedEffort:
              component === 'stalker' || component === 'person-fabric' ? 'medium' : 'high',
          },
        ]),
      ),
      formatAttempts: 2,
      stageTimeoutMs: 600000,
      promptVersion: 'bsc-v2.4-camel-case',
      createdAt: '2026-11-08T09:00:00.000Z',
      sourceSnapshots: [['snapshot-a-1'], ['snapshot-b-1', 'snapshot-b-2']],
    },
    people: [first, second],
    control: neutral,
    trials: options.trials ?? [],
    comparison: { summary: '', difference: null, comparisonEligible: false },
    finishedAt: '2026-11-08T11:00:00.000Z',
    traces: [...(options.traces ?? [])],
  };
}

function fullStages(rounds: number): StageLifecycle[] {
  const stages: StageLifecycle[] = [
    lifecycle('rubric', 'brain-analyzator', null, 'completed', [call()]),
    lifecycle('sources-0', 'stalker', 0, 'completed', [call({ operation: 'public-research' })]),
    lifecycle('profile-0', 'person-fabric', 0, 'completed', [call()]),
    lifecycle('sources-1', 'stalker', 1, 'completed', [call({ operation: 'public-research' })]),
    lifecycle('profile-1', 'person-fabric', 1, 'completed', [call()]),
    lifecycle('adapt-0', 'person-fabric', 0, 'completed', [call()]),
    lifecycle('adapt-1', 'person-fabric', 1, 'completed', [call()]),
    lifecycle('adapt-2', 'person-fabric', 2, 'completed', [call()]),
  ];
  for (let round = 1; round <= rounds; round++) {
    for (const [suffix, participant] of [
      ['a', 0],
      ['b', 1],
      ['neutral', 2],
    ] as const) {
      stages.push(
        lifecycle(`solve-${round}-${suffix}`, 'person', participant, 'completed', [
          call({ callId: `call-solve-${round}-${suffix}` }),
        ]),
      );
    }
    for (const pass of [0, 1] as const) {
      stages.push(
        lifecycle(`judge-${round}-${pass}`, 'brain-analyzator', null, 'completed', [
          call({ callId: `call-judge-${round}-${pass}` }),
        ]),
      );
    }
  }
  return stages;
}

describe('stage identity projection', () => {
  it.each([
    [{ kind: 'rubric' }, 'rubric'],
    [{ kind: 'research', person: 0 }, 'sources-0'],
    [{ kind: 'research', person: 1 }, 'sources-1'],
    [{ kind: 'profile', person: 0 }, 'profile-0'],
    [{ kind: 'profile', person: 1 }, 'profile-1'],
    [{ kind: 'adaptation', participant: 0 }, 'adapt-0'],
    [{ kind: 'adaptation', participant: 1 }, 'adapt-1'],
    [{ kind: 'adaptation', participant: 2 }, 'adapt-2'],
    [{ kind: 'solution', round: 1, participant: 'a' }, 'solve-1-a'],
    [{ kind: 'solution', round: 2, participant: 'b' }, 'solve-2-b'],
    [{ kind: 'solution', round: 3, participant: 'neutral' }, 'solve-3-neutral'],
    [{ kind: 'judgment', round: 1, pass: 0 }, 'judge-1-0'],
    [{ kind: 'judgment', round: 3, pass: 1 }, 'judge-3-1'],
  ] as const)('projects %j to the exact persisted stage id %s', (request, stageId) => {
    expect(stageArtifactStageId(request)).toBe(stageId);
  });

  it('enumerates exactly the planned stages for one repetition', () => {
    expect(stageArtifactRequests(result({ repetitions: 1 })).map(stageArtifactStageId)).toEqual([
      'rubric',
      'sources-0',
      'profile-0',
      'sources-1',
      'profile-1',
      'adapt-0',
      'adapt-1',
      'adapt-2',
      'solve-1-a',
      'solve-1-b',
      'solve-1-neutral',
      'judge-1-0',
      'judge-1-1',
    ]);
  });

  it('enumerates per-round solutions in the persisted participant order rotation', () => {
    const output = result({ repetitions: 3 });
    expect(stageArtifactRequests(output).map(stageArtifactStageId)).toEqual([
      'rubric',
      'sources-0',
      'profile-0',
      'sources-1',
      'profile-1',
      'adapt-0',
      'adapt-1',
      'adapt-2',
      'solve-1-a',
      'solve-1-b',
      'solve-1-neutral',
      'judge-1-0',
      'judge-1-1',
      'solve-2-b',
      'solve-2-neutral',
      'solve-2-a',
      'judge-2-0',
      'judge-2-1',
      'solve-3-neutral',
      'solve-3-a',
      'solve-3-b',
      'judge-3-0',
      'judge-3-1',
    ]);
  });

  it('follows a custom persisted plan order instead of the default rotation', () => {
    const output = result({ repetitions: 1, participantOrder: [['neutral', 'b', 'a']] });
    expect(stageArtifactRequests(output).map(stageArtifactStageId)).toEqual([
      'rubric',
      'sources-0',
      'profile-0',
      'sources-1',
      'profile-1',
      'adapt-0',
      'adapt-1',
      'adapt-2',
      'solve-1-neutral',
      'solve-1-b',
      'solve-1-a',
      'judge-1-0',
      'judge-1-1',
    ]);
  });

  it('falls back to the canonical participants when a persisted round order is absent', () => {
    const output = result({ repetitions: 1, participantOrder: [] });
    const ids = stageArtifactRequests(output).map(stageArtifactStageId);
    expect(ids).toContain('solve-1-a');
    expect(ids).toContain('solve-1-b');
    expect(ids).toContain('solve-1-neutral');
  });

  it('skips plan-order entries that are not known persisted participant ids', () => {
    const output = result({ repetitions: 1, participantOrder: [['ghost', 'a', 'b']] });
    const ids = stageArtifactRequests(output).map(stageArtifactStageId);
    expect(ids).toContain('solve-1-a');
    expect(ids).toContain('solve-1-b');
    expect(ids).not.toContain('solve-1-neutral');
    expect(ids).not.toContain('solve-1-ghost');
  });
});

describe('rubric and protocol artifact', () => {
  it('exposes the frozen task and rubric by reference with the stage location', () => {
    const stages = fullStages(1);
    const rubricTrace = stageTrace('rubric', 'brain-analyzator', null);
    const output = result({ stages, traces: [rubricTrace] });
    const artifact = ofKind(projectStageArtifact(output, { kind: 'rubric' }), 'rubric');
    expect(artifact.stageId).toBe('rubric');
    expect(artifact.stage.trace).toEqual({ status: 'available', value: rubricTrace });
    expect(artifact.stage.lifecycle).toEqual({ status: 'available', value: stages[0] });
    expect(artifact.stage.executionPresent).toBe(true);
    expect(artifact.rubric.task).toBe(taskText);
    expect(artifact.rubric.rubric).toBe(output.protocol.rubric);
    expect(artifact.classification).toEqual({
      participant: 'system',
      round: null,
      phase: 'protocol',
      componentLabel: 'Brain Analyzator',
    });
  });

  it('keeps the rubric artifact available even when the stage never produced a trace', () => {
    const output = result({ stages: [], execution: false });
    const artifact = ofKind(projectStageArtifact(output, { kind: 'rubric' }), 'rubric');
    expect(artifact.stage.trace).toEqual({ status: 'unavailable', reason: 'trace-not-found' });
    expect(artifact.stage.lifecycle).toEqual({
      status: 'unavailable',
      reason: 'lifecycle-not-found',
    });
    expect(artifact.rubric.rubric).toBe(output.protocol.rubric);
  });
});

describe('research report artifact', () => {
  it('resolves the person report by data index and links the granted source snapshots', () => {
    const output = result({ stages: fullStages(1) });
    const first = ofKind(projectStageArtifact(output, { kind: 'research', person: 0 }), 'research');
    expect(first.report.status).toBe('available');
    if (first.report.status !== 'available') return;
    expect(first.report.value.report).toBe(output.people[0]!.report);
    expect(first.report.value.sourceSnapshotIds).toEqual(['snapshot-a-1']);
    const second = ofKind(
      projectStageArtifact(output, { kind: 'research', person: 1 }),
      'research',
    );
    expect(second.report.status).toBe('available');
    if (second.report.status !== 'available') return;
    expect(second.report.value.report).toBe(output.people[1]!.report);
    expect(second.report.value.sourceSnapshotIds).toEqual(['snapshot-b-1', 'snapshot-b-2']);
  });

  it('reports an explicit unavailable state when the report was not produced', () => {
    const withoutReport = person('a', 'Персона A', 'synthetic', { report: null });
    const output = result({ people: [withoutReport, person('b', 'Персона B', 'synthetic')] });
    const artifact = ofKind(
      projectStageArtifact(output, { kind: 'research', person: 0 }),
      'research',
    );
    expect(artifact.report).toEqual({ status: 'unavailable', reason: 'artifact-not-produced' });
  });
});

describe('profile artifacts', () => {
  it('returns the task-free base profile for the profile stage', () => {
    const output = result({ stages: fullStages(1) });
    const artifact = ofKind(
      projectStageArtifact(output, { kind: 'profile', person: 1 }),
      'profile',
    );
    expect(artifact.profile.status).toBe('available');
    if (artifact.profile.status !== 'available') return;
    expect(artifact.profile.value).toBe(output.people[1]!.baseProfile);
    expect(artifact.profile.value).not.toBe(output.people[1]!.profile);
    expect(artifact.classification.phase).toBe('profile');
  });

  it('returns the adapted profiles for both persons and the neutral control', () => {
    const output = result({ stages: fullStages(1) });
    const first = ofKind(
      projectStageArtifact(output, { kind: 'adaptation', participant: 0 }),
      'adaptation',
    );
    expect(first.profile.status).toBe('available');
    if (first.profile.status !== 'available') return;
    expect(first.profile.value).toBe(output.people[0]!.profile);
    const second = ofKind(
      projectStageArtifact(output, { kind: 'adaptation', participant: 1 }),
      'adaptation',
    );
    expect(second.profile.status).toBe('available');
    if (second.profile.status !== 'available') return;
    expect(second.profile.value).toBe(output.people[1]!.profile);
    const control = ofKind(
      projectStageArtifact(output, { kind: 'adaptation', participant: 2 }),
      'adaptation',
    );
    expect(control.profile.status).toBe('available');
    if (control.profile.status !== 'available') return;
    expect(control.profile.value).toBe(output.control.profile);
    expect(control.classification.participant).toBe('control');
  });

  it('reports an unavailable profile when the participant is missing from the result', () => {
    const partial = { ...result({}), people: [person('a', 'Персона A', 'evidence-informed')] };
    const artifact = ofKind(
      projectStageArtifact(partial, { kind: 'profile', person: 1 }),
      'profile',
    );
    expect(artifact.profile).toEqual({ status: 'unavailable', reason: 'artifact-not-produced' });
  });
});

describe('solution trial artifact', () => {
  it('disambiguates rounds and persons by producer fields, never by labels', () => {
    const firstRound = trial(1, 'b', { status: 'answered', score: null, evaluations: [] });
    const secondRoundA = trial(2, 'a');
    const secondRoundB = trial(2, 'b', {
      attempt: {
        answer: 'Ответ B второго прогона',
        trace: trace(),
      },
    });
    const output = result({
      repetitions: 3,
      stages: fullStages(3),
      trials: [firstRound, secondRoundA, secondRoundB],
    });
    const artifact = ofKind(
      projectStageArtifact(output, { kind: 'solution', round: 2, participant: 'b' }),
      'solution',
    );
    expect(artifact.stageId).toBe('solve-2-b');
    expect(artifact.classification).toEqual({
      participant: 'b',
      round: 2,
      phase: 'solution',
      componentLabel: 'Person',
    });
    expect(artifact.solution.status).toBe('available');
    if (artifact.solution.status !== 'available') return;
    expect(artifact.solution.value.trial).toBe(secondRoundB);
    expect(artifact.solution.value.answer).toEqual({
      status: 'available',
      value: 'Ответ B второго прогона',
    });
  });

  it('preserves exact answer text with whitespace and Unicode by reference', () => {
    const exact = 'Первая часть ответа\n\nвторая часть с\u00A0неразрывным пробелом · ユニコード';
    const solved = trial(1, 'a', {
      attempt: {
        answer: exact,
        trace: trace(),
        reply: { version: 2, messages: [exact], styleConflicts: [] },
      },
    });
    const output = result({ trials: [solved] });
    const artifact = ofKind(
      projectStageArtifact(output, { kind: 'solution', round: 1, participant: 'a' }),
      'solution',
    );
    expect(artifact.solution.status).toBe('available');
    if (artifact.solution.status !== 'available') return;
    expect(artifact.solution.value.answer.status).toBe('available');
    if (artifact.solution.value.answer.status !== 'available') return;
    expect(artifact.solution.value.answer.value).toBe(exact);
    expect(artifact.solution.value.reply.status).toBe('available');
    if (artifact.solution.value.reply.status !== 'available') return;
    expect(artifact.solution.value.reply.value.messages).toEqual([exact]);
  });

  it('keeps the answer available without reasoning and exposes the trial score when present', () => {
    const solved = trial(1, 'a', {
      attempt: {
        answer: 'Ответ без рассуждений',
        trace: trace({ reasoning: [], summary: 'CLI не передал reasoning' }),
      },
      score: {
        version: 2,
        personId: 'a',
        interval: { lower: 80, upper: 100 },
        criteria: [],
        requiredStatus: 'passed',
        judgeDisagreement: false,
      },
    });
    const output = result({ trials: [solved] });
    const artifact = ofKind(
      projectStageArtifact(output, { kind: 'solution', round: 1, participant: 'a' }),
      'solution',
    );
    expect(artifact.solution.status).toBe('available');
    if (artifact.solution.status !== 'available') return;
    expect(artifact.solution.value.answer.status).toBe('available');
    expect(artifact.solution.value.score.status).toBe('available');
    if (artifact.solution.value.score.status !== 'available') return;
    expect(artifact.solution.value.score.value.interval).toEqual({ lower: 80, upper: 100 });
  });

  it('states the reply as not persisted instead of synthesizing messages', () => {
    const legacy = trial(1, 'a', {
      attempt: { answer: 'Наследованный ответ без reply', trace: trace() },
    });
    const output = result({ trials: [legacy] });
    const artifact = ofKind(
      projectStageArtifact(output, { kind: 'solution', round: 1, participant: 'a' }),
      'solution',
    );
    expect(artifact.solution.status).toBe('available');
    if (artifact.solution.status !== 'available') return;
    expect(artifact.solution.value.answer.status).toBe('available');
    expect(artifact.solution.value.reply).toEqual({
      status: 'unavailable',
      reason: 'reply-not-persisted',
    });
  });

  it('reports explicit unavailable states for skipped and failed trials', () => {
    const skipped = trial(1, 'b', {
      status: 'skipped',
      attempt: null,
      error: 'Серия отменена',
    });
    const failed = trial(1, 'a', {
      status: 'failed',
      attempt: null,
      error: 'Провайдер недоступен',
    });
    const output = result({ trials: [skipped, failed] });
    const skippedArtifact = ofKind(
      projectStageArtifact(output, { kind: 'solution', round: 1, participant: 'b' }),
      'solution',
    );
    expect(skippedArtifact.solution.status).toBe('available');
    if (skippedArtifact.solution.status !== 'available') return;
    expect(skippedArtifact.solution.value.answer).toEqual({
      status: 'unavailable',
      reason: 'trial-skipped',
    });
    expect(skippedArtifact.solution.value.score).toEqual({
      status: 'unavailable',
      reason: 'not-scored',
    });
    expect(skippedArtifact.solution.value.trial.error).toBe('Серия отменена');
    const failedArtifact = ofKind(
      projectStageArtifact(output, { kind: 'solution', round: 1, participant: 'a' }),
      'solution',
    );
    expect(failedArtifact.solution.status).toBe('available');
    if (failedArtifact.solution.status !== 'available') return;
    expect(failedArtifact.solution.value.answer).toEqual({
      status: 'unavailable',
      reason: 'trial-failed',
    });
  });

  it('reports an unplanned round as an unavailable trial instead of inventing data', () => {
    const output = result({ repetitions: 1, trials: [trial(1, 'a')] });
    const artifact = ofKind(
      projectStageArtifact(output, { kind: 'solution', round: 2, participant: 'a' }),
      'solution',
    );
    expect(artifact.solution).toEqual({
      status: 'unavailable',
      reason: 'trial-not-planned',
    });
  });

  it('reports duplicate trials for the same round and person as unavailable', () => {
    const first = trial(1, 'a');
    const duplicate = trial(1, 'a', {
      attempt: { answer: 'Другой ответ того же прогона', trace: trace() },
    });
    const output = result({ trials: [first, duplicate] });
    const artifact = ofKind(
      projectStageArtifact(output, { kind: 'solution', round: 1, participant: 'a' }),
      'solution',
    );
    expect(artifact.solution).toEqual({ status: 'unavailable', reason: 'duplicate-trials' });
  });
});

describe('judgment artifact', () => {
  it('maps the forward and reverse passes by stored evaluation order', () => {
    const forwardB = evaluation('b', 'цитата-b-прямой-порядок');
    const reverseB = evaluation('b', 'цитата-b-обратный-порядок');
    const forwardNeutral = evaluation('neutral', 'цитата-neutral-прямой-порядок');
    const reverseNeutral = evaluation('neutral', 'цитата-neutral-обратный-порядок');
    const output = result({
      repetitions: 1,
      trials: [
        trial(1, 'a', {
          evaluations: [evaluation('a', 'цитата-a-прямой'), evaluation('a', 'цитата-a-обратный')],
        }),
        trial(1, 'b', { evaluations: [forwardB, reverseB] }),
        trial(1, 'neutral', { evaluations: [forwardNeutral, reverseNeutral] }),
      ],
    });
    const forward = ofKind(
      projectStageArtifact(output, { kind: 'judgment', round: 1, pass: 0 }),
      'judgment',
    );
    expect(forward.judgment.status).toBe('available');
    if (forward.judgment.status !== 'available') return;
    expect(forward.judgment.value.pass).toBe(0);
    expect(forward.judgment.value.assessments).toHaveLength(3);
    const byPerson = new Map(
      forward.judgment.value.assessments.map((entry) => [entry.personId, entry.evaluation]),
    );
    expect(byPerson.get('b')?.criteria[0]?.quote).toBe('цитата-b-прямой-порядок');
    expect(byPerson.get('neutral')?.criteria[0]?.quote).toBe('цитата-neutral-прямой-порядок');

    const reverse = ofKind(
      projectStageArtifact(output, { kind: 'judgment', round: 1, pass: 1 }),
      'judgment',
    );
    expect(reverse.judgment.status).toBe('available');
    if (reverse.judgment.status !== 'available') return;
    expect(reverse.judgment.value.pass).toBe(1);
    const reverseByPerson = new Map(
      reverse.judgment.value.assessments.map((entry) => [entry.personId, entry.evaluation]),
    );
    expect(reverseByPerson.get('b')?.criteria[0]?.quote).toBe('цитата-b-обратный-порядок');
  });

  it('keeps partially evaluated persons explicit inside an available judgment', () => {
    const output = result({
      repetitions: 1,
      trials: [
        trial(1, 'a', { evaluations: [evaluation('a', 'цитата-a')] }),
        trial(1, 'b', { evaluations: [] }),
      ],
    });
    const artifact = ofKind(
      projectStageArtifact(output, { kind: 'judgment', round: 1, pass: 0 }),
      'judgment',
    );
    expect(artifact.judgment.status).toBe('available');
    if (artifact.judgment.status !== 'available') return;
    const byPerson = new Map(
      artifact.judgment.value.assessments.map((entry) => [entry.personId, entry.evaluation]),
    );
    expect(byPerson.get('a')?.criteria[0]?.quote).toBe('цитата-a');
    expect(byPerson.get('b')).toBeNull();
  });

  it('reports an explicit unavailable state when the judge pass was never evaluated', () => {
    const output = result({
      repetitions: 1,
      trials: [trial(1, 'a', { evaluations: [] })],
    });
    const artifact = ofKind(
      projectStageArtifact(output, { kind: 'judgment', round: 1, pass: 1 }),
      'judgment',
    );
    expect(artifact.judgment).toEqual({
      status: 'unavailable',
      reason: 'evaluations-not-produced',
    });
    const unplanned = ofKind(
      projectStageArtifact(output, { kind: 'judgment', round: 3, pass: 0 }),
      'judgment',
    );
    expect(unplanned.judgment).toEqual({
      status: 'unavailable',
      reason: 'evaluations-not-produced',
    });
  });

  it('covers the producer partial series: forward pass stored, reverse pass stopped', () => {
    // runTrials stops in its outer catch when a judge stage fails, so a round can only
    // persist the forward evaluations without the reverse ones, never the opposite.
    const output = result({
      repetitions: 1,
      trials: [
        trial(1, 'a', { evaluations: [evaluation('a', 'цитата-a-прямой')] }),
        trial(1, 'b', { evaluations: [evaluation('b', 'цитата-b-прямой')] }),
        trial(1, 'neutral', { evaluations: [evaluation('neutral', 'цитата-neutral-прямой')] }),
      ],
    });
    const forward = ofKind(
      projectStageArtifact(output, { kind: 'judgment', round: 1, pass: 0 }),
      'judgment',
    );
    expect(forward.judgment.status).toBe('available');
    if (forward.judgment.status !== 'available') return;
    expect(forward.judgment.value.assessments).toHaveLength(3);
    const reverse = ofKind(
      projectStageArtifact(output, { kind: 'judgment', round: 1, pass: 1 }),
      'judgment',
    );
    expect(reverse.judgment).toEqual({
      status: 'unavailable',
      reason: 'evaluations-not-produced',
    });
  });

  it('refuses to attribute an evaluation stored under another person id', () => {
    const output = result({
      repetitions: 1,
      trials: [trial(1, 'a', { evaluations: [evaluation('b', 'цитата чужой персоны')] })],
    });
    const artifact = ofKind(
      projectStageArtifact(output, { kind: 'judgment', round: 1, pass: 0 }),
      'judgment',
    );
    expect(artifact.judgment).toEqual({
      status: 'unavailable',
      reason: 'evaluation-person-mismatch',
    });
    const solution = ofKind(
      projectStageArtifact(output, { kind: 'solution', round: 1, participant: 'a' }),
      'solution',
    );
    expect(solution.solution.status).toBe('available');
  });

  it('reports duplicate trials in a round as an unavailable judgment', () => {
    const output = result({ repetitions: 1, trials: [trial(1, 'a'), trial(1, 'a')] });
    const artifact = ofKind(
      projectStageArtifact(output, { kind: 'judgment', round: 1, pass: 0 }),
      'judgment',
    );
    expect(artifact.judgment).toEqual({ status: 'unavailable', reason: 'duplicate-trials' });
  });
});

describe('calls and errors projection', () => {
  it('resolves per-call reason, type, status and duration for a format-repaired solve', () => {
    const repairedLifecycle = lifecycle('solve-1-a', 'person', 0, 'completed', [
      call({
        callId: 'call-1',
        operation: 'persona-reply',
        trigger: 'initial',
        formatAttempt: 1,
        status: 'failed',
        wallTimeMs: 900,
        validationError: 'Response failed format validation',
      }),
      call({
        callId: 'call-2',
        operation: 'persona-reply',
        trigger: 'formatRepair',
        formatAttempt: 2,
        status: 'completed',
        wallTimeMs: 1100,
      }),
    ]);
    const repairedTrace = stageTrace('solve-1-a', 'person', 0, {
      trace: trace({ wallTimeMs: 2000, reasoningMs: 2000 }),
      attempts: [
        { index: 1, status: 'format-rejected', wallTimeMs: 900, error: null },
        { index: 2, status: 'accepted', wallTimeMs: 1100, error: null },
      ],
    });
    const output = result({
      stages: [repairedLifecycle],
      traces: [repairedTrace],
      trials: [trial(1, 'a')],
    });
    const artifact = ofKind(
      projectStageArtifact(output, { kind: 'solution', round: 1, participant: 'a' }),
      'solution',
    );
    expect(artifact.calls.status).toBe('available');
    if (artifact.calls.status !== 'available') return;
    expect(artifact.calls.calls).toHaveLength(2);
    expect(artifact.calls.calls[0]).toEqual({
      index: 1,
      callId: 'call-1',
      operation: 'persona-reply',
      trigger: 'initial',
      formatAttempt: 1,
      status: 'failed',
      attemptStatus: 'format-rejected',
      wallTimeMs: 900,
      error: null,
      validationError: 'Response failed format validation',
    });
    expect(artifact.calls.calls[1]).toEqual({
      index: 2,
      callId: 'call-2',
      operation: 'persona-reply',
      trigger: 'formatRepair',
      formatAttempt: 2,
      status: 'completed',
      attemptStatus: 'accepted',
      wallTimeMs: 1100,
      error: null,
      validationError: null,
    });
  });

  it('uses per-call durations instead of the cumulative trace duration', () => {
    const stage = lifecycle('sources-0', 'stalker', 0, 'completed', [
      call({ callId: 'call-a', wallTimeMs: 400 }),
      call({ callId: 'call-b', wallTimeMs: 600 }),
    ]);
    const output = result({
      stages: [stage],
      traces: [
        stageTrace('sources-0', 'stalker', 0, {
          trace: trace({ wallTimeMs: 10000 }),
          attempts: [
            { index: 1, status: 'accepted', wallTimeMs: 400, error: null },
            { index: 2, status: 'accepted', wallTimeMs: 600, error: null },
          ],
        }),
      ],
    });
    const artifact = ofKind(
      projectStageArtifact(output, { kind: 'research', person: 0 }),
      'research',
    );
    expect(artifact.calls.status).toBe('available');
    if (artifact.calls.status !== 'available') return;
    expect(artifact.calls.calls.map((entry) => entry.wallTimeMs)).toEqual([400, 600]);
  });

  it('reports legacy-unknown calls for results without execution metadata', () => {
    const output = result({
      execution: false,
      traces: [
        stageTrace('solve-1-a', 'person', 0, {
          attempts: [{ index: 1, status: 'accepted', wallTimeMs: 1000, error: null }],
        }),
      ],
      trials: [trial(1, 'a')],
    });
    const artifact = ofKind(
      projectStageArtifact(output, { kind: 'solution', round: 1, participant: 'a' }),
      'solution',
    );
    expect(artifact.stage.executionPresent).toBe(false);
    expect(artifact.stage.lifecycle).toEqual({
      status: 'unavailable',
      reason: 'lifecycle-not-found',
    });
    expect(artifact.calls).toEqual({
      status: 'unavailable',
      reason: 'legacy-execution-metadata-missing',
    });
  });

  it('reports an unrecorded stage explicitly while execution metadata exists', () => {
    const output = result({
      stages: [lifecycle('rubric', 'brain-analyzator', null, 'completed', [])],
    });
    const artifact = ofKind(
      projectStageArtifact(output, { kind: 'profile', person: 1 }),
      'profile',
    );
    expect(artifact.stage.executionPresent).toBe(true);
    expect(artifact.stage.lifecycle).toEqual({
      status: 'unavailable',
      reason: 'lifecycle-not-found',
    });
    expect(artifact.calls).toEqual({ status: 'unavailable', reason: 'stage-not-recorded' });
  });

  it('keeps a skipped stage resolvable through lifecycle only, with zero calls', () => {
    const skipped = lifecycle('solve-1-neutral', 'person', 2, 'skipped', [], 'Серия отменена');
    const output = result({
      stages: [skipped],
      trials: [trial(1, 'neutral', { status: 'skipped', attempt: null })],
    });
    const artifact = ofKind(
      projectStageArtifact(output, { kind: 'solution', round: 1, participant: 'neutral' }),
      'solution',
    );
    expect(artifact.stage.trace).toEqual({ status: 'unavailable', reason: 'trace-not-found' });
    expect(artifact.stage.lifecycle).toEqual({ status: 'available', value: skipped });
    expect(artifact.classification.participant).toBe('control');
    expect(artifact.calls).toEqual({ status: 'available', calls: [] });
  });

  it.each([
    [
      'reordered attempt indices',
      [
        { index: 2, status: 'accepted', wallTimeMs: 1100, error: null },
        { index: 1, status: 'format-rejected', wallTimeMs: 900, error: null },
      ],
    ],
    [
      'duplicated attempt indices',
      [
        { index: 1, status: 'format-rejected', wallTimeMs: 900, error: null },
        { index: 1, status: 'accepted', wallTimeMs: 1100, error: null },
      ],
    ],
    [
      'an attempt beyond the recorded calls',
      [
        { index: 1, status: 'accepted', wallTimeMs: 900, error: null },
        { index: 2, status: 'accepted', wallTimeMs: 1100, error: null },
      ],
    ],
  ] as const)('never attributes another attempt to a call on %s', (_name, attempts) => {
    const stage = lifecycle('solve-1-a', 'person', 0, 'completed', [call({ callId: 'call-1' })]);
    const output = result({
      stages: [stage],
      traces: [stageTrace('solve-1-a', 'person', 0, { attempts })],
      trials: [trial(1, 'a')],
    });
    const artifact = ofKind(
      projectStageArtifact(output, { kind: 'solution', round: 1, participant: 'a' }),
      'solution',
    );
    expect(artifact.calls).toEqual({ status: 'unavailable', reason: 'attempt-index-mismatch' });
  });
});

describe('stage location ambiguity', () => {
  it('marks duplicate same-id traces as ambiguous instead of silently choosing one', () => {
    const first = stageTrace('solve-1-a', 'person', 0, { trace: trace({ wallTimeMs: 1000 }) });
    const second = stageTrace('solve-1-a', 'person', 0, { trace: trace({ wallTimeMs: 9999 }) });
    const stage = lifecycle('solve-1-a', 'person', 0, 'completed', [call()]);
    const output = result({
      stages: [stage],
      traces: [first, second],
      trials: [trial(1, 'a')],
    });
    const artifact = ofKind(
      projectStageArtifact(output, { kind: 'solution', round: 1, participant: 'a' }),
      'solution',
    );
    expect(artifact.stage.trace).toEqual({
      status: 'unavailable',
      reason: 'duplicate-stage-traces',
    });
    expect(artifact.stage.lifecycle).toEqual({ status: 'available', value: stage });
    expect(artifact.calls).toEqual({ status: 'unavailable', reason: 'duplicate-stage-traces' });
    expect(artifact.solution.status).toBe('available');
  });

  it('marks duplicate same-id lifecycle stages as ambiguous', () => {
    const first = lifecycle('solve-1-a', 'person', 0, 'completed', [call({ callId: 'call-1' })]);
    const second = lifecycle('solve-1-a', 'person', 0, 'completed', [call({ callId: 'call-2' })]);
    const aTrace = stageTrace('solve-1-a', 'person', 0);
    const output = result({
      stages: [first, second],
      traces: [aTrace],
      trials: [trial(1, 'a')],
    });
    const artifact = ofKind(
      projectStageArtifact(output, { kind: 'solution', round: 1, participant: 'a' }),
      'solution',
    );
    expect(artifact.stage.lifecycle).toEqual({
      status: 'unavailable',
      reason: 'duplicate-stage-lifecycles',
    });
    expect(artifact.stage.trace).toEqual({ status: 'available', value: aTrace });
    expect(artifact.calls).toEqual({
      status: 'unavailable',
      reason: 'duplicate-stage-lifecycles',
    });
  });

  it('refuses contradictory trace and execution identities for the same stage id', () => {
    const stage = lifecycle('solve-1-a', 'person', 0, 'completed', [call()]);
    const contradictory = stageTrace('solve-1-a', 'stalker', 1);
    const output = result({
      stages: [stage],
      traces: [contradictory],
      trials: [trial(1, 'a')],
    });
    const artifact = ofKind(
      projectStageArtifact(output, { kind: 'solution', round: 1, participant: 'a' }),
      'solution',
    );
    expect(artifact.stage.trace).toEqual({
      status: 'unavailable',
      reason: 'stage-identity-mismatch',
    });
    expect(artifact.stage.lifecycle).toEqual({
      status: 'unavailable',
      reason: 'stage-identity-mismatch',
    });
    expect(artifact.calls).toEqual({ status: 'unavailable', reason: 'stage-identity-mismatch' });
    expect(artifact.classification).toEqual({
      participant: 'a',
      round: 1,
      phase: 'solution',
      componentLabel: 'Person',
    });
  });

  it('refuses a trace whose embedded lifecycle belongs to another stage id', () => {
    const embedded = lifecycle('solve-1-b', 'person', 1, 'completed', []);
    const aTrace = stageTrace('solve-1-a', 'person', 0, { lifecycle: embedded });
    const stage = lifecycle('solve-1-a', 'person', 0, 'completed', [call()]);
    const output = result({ stages: [stage], traces: [aTrace], trials: [trial(1, 'a')] });
    const artifact = ofKind(
      projectStageArtifact(output, { kind: 'solution', round: 1, participant: 'a' }),
      'solution',
    );
    expect(artifact.stage.trace).toEqual({
      status: 'unavailable',
      reason: 'stage-identity-mismatch',
    });
    expect(artifact.stage.lifecycle).toEqual({
      status: 'unavailable',
      reason: 'stage-identity-mismatch',
    });
    expect(artifact.calls).toEqual({ status: 'unavailable', reason: 'stage-identity-mismatch' });
  });

  it('resolves the lifecycle from a consistent embedded trace lifecycle when execution lacks the stage', () => {
    const embedded = lifecycle('solve-1-a', 'person', 0, 'completed', [call()]);
    const aTrace = stageTrace('solve-1-a', 'person', 0, { lifecycle: embedded });
    const output = result({
      stages: [lifecycle('rubric', 'brain-analyzator', null, 'completed', [])],
      traces: [aTrace],
      trials: [trial(1, 'a')],
    });
    const artifact = ofKind(
      projectStageArtifact(output, { kind: 'solution', round: 1, participant: 'a' }),
      'solution',
    );
    expect(artifact.stage.lifecycle).toEqual({ status: 'available', value: embedded });
    expect(artifact.calls.status).toBe('available');
  });
});

describe('recall receipt seam', () => {
  it('states that recall receipts are not persisted for any stage', () => {
    const output = result({ stages: fullStages(1), trials: [trial(1, 'a')] });
    for (const request of stageArtifactRequests(output)) {
      const artifact = projectStageArtifact(output, request);
      expect(artifact.receipts).toEqual({
        status: 'unavailable',
        reason: 'recall-receipts-not-persisted',
      });
    }
  });
});

describe('projection integrity', () => {
  it('returns persisted objects by reference without copying or mutating the result', () => {
    const report = {
      evidence: [],
      character: 'c',
      thinkingStyle: 't',
      activity: 'a',
      behaviorSummary: 'b',
    };
    const second = person('b', 'Персона B', 'synthetic', { report });
    const output = result({
      stages: fullStages(1),
      traces: [stageTrace('sources-1', 'stalker', 1)],
      trials: [trial(1, 'a')],
      people: [person('a', 'Персона A', 'evidence-informed'), second],
    });
    const frozen = structuredClone(output);
    const artifact = ofKind(
      projectStageArtifact(output, { kind: 'research', person: 1 }),
      'research',
    );
    expect(artifact.report.status).toBe('available');
    if (artifact.report.status !== 'available') return;
    expect(artifact.report.value.report).toBe(report);
    expect(output).toEqual(frozen);
  });

  it('keeps requested settings and observed fields on the referenced trace untouched', () => {
    const output = result({
      stages: fullStages(1),
      traces: [
        stageTrace('solve-1-a', 'person', 0, {
          trace: trace({
            requestedModel: 'opus',
            appliedEffort: 'high',
            resolvedModel: null,
            cliVersion: null,
          }),
        }),
      ],
      trials: [trial(1, 'a')],
    });
    const artifact = ofKind(
      projectStageArtifact(output, { kind: 'solution', round: 1, participant: 'a' }),
      'solution',
    );
    const located = artifact.stage.trace;
    expect(located.status).toBe('available');
    if (located.status !== 'available') return;
    expect(located.value.trace.requestedModel).toBe('opus');
    expect(located.value.trace.resolvedModel).toBeNull();
    expect(located.value.trace.cliVersion).toBeNull();
    expect(located.value.trace.activeReasoningMs).toBeNull();
  });

  it('classifies from the located lifecycle identity instead of the trace fallback', () => {
    const located = lifecycle('solve-2-neutral', 'person', 2, 'completed', []);
    const output = result({
      stages: [located],
      traces: [stageTrace('old-id', 'stalker', 0)],
    });
    const artifact = ofKind(
      projectStageArtifact(output, { kind: 'solution', round: 2, participant: 'neutral' }),
      'solution',
    );
    expect(artifact.classification).toEqual({
      participant: 'control',
      round: 2,
      phase: 'solution',
      componentLabel: 'Person',
    });
  });
});

describe('live and durable call identity', () => {
  it('rejects duplicate call IDs even when attempt indices are sequential', () => {
    const calls = [call(), call()];
    const output = result({
      stages: [lifecycle('solve-1-a', 'person', 0, 'completed', calls)],
      traces: [
        stageTrace('solve-1-a', 'person', 0, {
          attempts: [
            { index: 1, status: 'accepted', wallTimeMs: 1, error: null },
            { index: 2, status: 'accepted', wallTimeMs: 2, error: null },
          ],
        }),
      ],
    });
    expect(
      projectStageArtifact(output, { kind: 'solution', round: 1, participant: 'a' }).calls,
    ).toEqual({ status: 'unavailable', reason: 'duplicate-call-ids' });
  });
  it('rejects a trace lifecycle from another run even when its logical stage matches', () => {
    const stage = lifecycle('solve-1-a', 'person', 0, 'completed', [call()]);
    const output = result({
      stages: [stage],
      traces: [
        stageTrace('solve-1-a', 'person', 0, {
          lifecycle: { ...stage, runId: 'other-run' },
        }),
      ],
    });
    expect(
      projectStageArtifact(output, { kind: 'solution', round: 1, participant: 'a' }).calls,
    ).toEqual({ status: 'unavailable', reason: 'stage-identity-mismatch' });
  });
});
