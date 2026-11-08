import { describe, expect, it } from 'vitest';
import type { StageTrace } from '@proteus-int/pipeline';
import { publishedStageText, stageCallsText, stageResultText } from './stage-artifact-content';
import type { StageArtifact } from './stage-artifacts';

const trace: StageTrace = {
  id: 'solve-1-a',
  stage: 'person',
  person: 0,
  label: 'Решение',
  attempts: [],
  trace: { provider: 'codex', reasoningMs: 0, terminalMs: 0, summary: '' },
};
const base = {
  stageId: 'solve-1-a',
  classification: { participant: 'a', round: 1, phase: 'solution', componentLabel: 'Person' },
  stage: {
    executionPresent: false,
    trace: { status: 'unavailable', reason: 'trace-not-found' },
    lifecycle: { status: 'unavailable', reason: 'lifecycle-not-found' },
  },
  calls: { status: 'unavailable', reason: 'legacy-execution-metadata-missing' },
  receipts: { status: 'unavailable', reason: 'recall-receipts-not-persisted' },
} as const;

function solution(answer: string): StageArtifact {
  return {
    ...base,
    request: { kind: 'solution', round: 1, participant: 'a' },
    solution: {
      status: 'available',
      value: {
        trial: {
          id: '1-a',
          round: 1,
          personId: 'a',
          status: 'answered',
          attempt: { answer, trace: trace.trace },
          score: null,
          evaluations: [],
          error: null,
        },
        answer: { status: 'available', value: answer },
        reply: { status: 'unavailable', reason: 'reply-not-persisted' },
        score: { status: 'unavailable', reason: 'not-scored' },
      },
    },
  };
}

describe('stage artifact presentation', () => {
  it('preserves exact answer bytes without rendering or adding role disclaimers', () => {
    const answer = '  Точный ответ\r\n\t🙂 <b>quoted</b>\u00a0';
    expect(publishedStageText(solution(answer))).toBe(answer);
    expect(stageResultText(solution(answer))).toContain('Структура сообщений не сохранена');
  });
  it('distinguishes missing requested parameters from an explicit CLI default', () => {
    expect(stageCallsText(undefined, trace)).toContain('Запрошенная модель: не сохранена');
    expect(
      stageCallsText(undefined, { ...trace, trace: { ...trace.trace, requestedModel: '' } }),
    ).toContain('Запрошенная модель: по умолчанию CLI');
    expect(stageCallsText(undefined, trace)).toContain('Наблюдаемая модель: не раскрыта');
  });
  it('shows a stage failure even if no model call was started', () => {
    const artifact: StageArtifact = {
      ...solution(''),
      stage: {
        executionPresent: true,
        trace: { status: 'unavailable', reason: 'trace-not-found' },
        lifecycle: {
          status: 'available',
          value: {
            schemaVersion: 1,
            runId: 'run',
            stageId: 'stage',
            logicalStageId: 'solve-1-a',
            component: 'person',
            person: 0,
            label: 'Решение',
            status: 'failed',
            startedAt: null,
            finishedAt: null,
            error: 'Разрешение отозвано\n  точная причина',
            calls: [],
          },
        },
      },
      calls: { status: 'available', calls: [] },
    };
    expect(stageCallsText(artifact, trace)).toContain(
      'Ошибка этапа: Разрешение отозвано\n  точная причина',
    );
    expect(stageCallsText(artifact, trace)).toContain('Вызовы модели не зарегистрированы');
  });
  it('does not synthesize artifact content from trace summaries or unknown identities', () => {
    expect(publishedStageText(undefined)).toContain('не сохранён');
    expect(stageResultText(undefined)).toContain('не сохранён');
    expect(
      stageCallsText(undefined, {
        ...trace,
        trace: { ...trace.trace, summary: 'unpersisted-answer' },
      }),
    ).not.toContain('unpersisted-answer');
  });
});

it('shows observed live calls before an ExperimentResult exists', () => {
  const live: StageTrace = {
    ...trace,
    lifecycle: {
      schemaVersion: 1,
      runId: 'run',
      stageId: 'stage',
      logicalStageId: trace.id,
      component: trace.stage,
      person: trace.person,
      label: trace.label,
      status: 'running',
      startedAt: null,
      finishedAt: null,
      error: null,
      calls: [
        {
          callId: 'live-call',
          operation: 'generation',
          trigger: 'initial',
          formatAttempt: 1,
          status: 'running',
          startedAt: 'now',
          returnedAt: null,
          finishedAt: null,
          wallTimeMs: null,
          error: null,
          validationError: null,
        },
      ],
    },
  };
  const text = stageCallsText(undefined, live);
  expect(text).toContain('live-call');
  expect(text).toContain('Статус: выполняется; время: не измерено');
  expect(
    stageCallsText(undefined, {
      ...live,
      lifecycle: { ...live.lifecycle!, logicalStageId: 'other-stage' },
    }),
  ).not.toContain('live-call');
});
