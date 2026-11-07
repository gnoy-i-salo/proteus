import { expect, it } from 'vitest';
import type { ExperimentPerson, ExperimentResult } from '@proteus-int/pipeline';
import type { ExperimentProtocol } from '@proteus-int/contracts';
import {
  reportText,
  formatInterval,
  formatScore,
  protocolText,
  traceMarker,
  formatRetryCount,
} from './experiment-report';

it('keeps a narrow uncertainty interval visible by rounding its limits outward', () => {
  expect(formatInterval({ lower: 0.01, upper: 0.04 })).toBe('0–0.1');
  expect(formatInterval({ lower: 65, upper: 85 })).toBe('65–85');
  expect(formatInterval({ lower: 100, upper: 100 })).toBe('100');
});
it('marks collapsed median intervals as uncertain when no complete point score is available', () => {
  expect(formatScore({ interval: { lower: 50, upper: 50 }, point: null })).toBe('≈50');
  expect(formatScore({ interval: { lower: 100, upper: 100 }, point: 100 })).toBe('100');
  expect(formatScore({ interval: { lower: 65, upper: 85 }, point: null })).toBe('65–85');
});
it('does not show a completion mark for a failed stage with an error summary', () => {
  const trace = {
    provider: 'codex' as const,
    reasoningMs: 10,
    terminalMs: 0,
    summary: 'Запрос не завершён',
  };
  expect(
    traceMarker(
      {
        trace,
        attempts: [{ index: 1, status: 'failed', wallTimeMs: 10, error: 'CLI failed' }],
      },
      false,
    ),
  ).toBe('!');
  expect(
    traceMarker(
      {
        trace,
        attempts: [{ index: 1, status: 'format-rejected', wallTimeMs: 10, error: null }],
      },
      false,
    ),
  ).toBe('!');
  expect(
    traceMarker(
      { trace, attempts: [{ index: 1, status: 'accepted', wallTimeMs: 10, error: null }] },
      false,
    ),
  ).toBe('✓');
  expect(traceMarker({ trace: { ...trace, summary: '' }, attempts: [] }, true)).toBe('◇');
});
it('shows small positive criterion weights without rounding them to zero', () => {
  const protocol: ExperimentProtocol = {
    version: 3,
    task: 'Task',
    repetitions: 1,
    participantOrder: [['a', 'b', 'neutral']],
    rubric: {
      version: 2,
      criteria: [
        {
          id: 'coverage',
          label: 'Coverage',
          weight: 0.01,
          required: false,
          anchors: { unmet: 'Missing', partial: 'Partial', met: 'Complete' },
        },
      ],
    },
    modelSettings: {},
    formatAttempts: 2,
    stageTimeoutMs: 600000,
    promptVersion: 'bsc-v2.2-source-style',
    sourceSnapshots: [[], []],
    createdAt: '2026-11-06T00:00:00Z',
  };
  expect(protocolText(protocol)).toContain('Вес: 0.01;');
});

it('does not count ordinary model calls as retries and keeps historical counts unknown', () => {
  const attempts = [1, 2, 3].map((index) => ({
    index,
    status: 'accepted' as const,
    wallTimeMs: 1,
    error: null,
  }));
  const lifecycle = {
    schemaVersion: 1 as const,
    runId: 'run',
    stageId: 'stage',
    logicalStageId: 'sources',
    component: 'stalker',
    person: 0,
    label: 'Sources',
    status: 'completed' as const,
    startedAt: '2026-11-07T00:00:00.000Z',
    finishedAt: '2026-11-07T00:00:01.000Z',
    error: null,
    calls: [1, 2, 3].map((index) => ({
      callId: `call-${index}`,
      operation: 'source-plan' as const,
      trigger: 'initial' as const,
      formatAttempt: 1 as const,
      status: 'completed' as const,
      startedAt: '2026-11-07T00:00:00.000Z',
      returnedAt: '2026-11-07T00:00:01.000Z',
      finishedAt: '2026-11-07T00:00:01.000Z',
      wallTimeMs: 1,
      error: null,
      validationError: null,
    })),
  };
  expect(formatRetryCount([{ attempts, lifecycle }])).toBe(0);
  expect(formatRetryCount([{ attempts }])).toBeNull();
  const repaired = {
    ...lifecycle,
    calls: [
      ...lifecycle.calls,
      {
        ...lifecycle.calls[0]!,
        callId: 'repair',
        trigger: 'formatRepair' as const,
        formatAttempt: 2 as const,
      },
    ],
  };
  expect(formatRetryCount([{ attempts, lifecycle: repaired }])).toBe(1);
});

it('keeps historical control retry counts unknown instead of reading an empty persona lane', () => {
  const profile = {
    id: 'neutral',
    label: 'Контроль',
    palette: 'turquoise' as const,
    archetype: 'Neutral',
    skills: [],
    rules: [],
    taskSkills: [],
    sourceStatus: 'neutral' as const,
  };
  const control: ExperimentPerson = {
    report: null,
    baseProfile: profile,
    profile,
    attempt: null,
    score: {
      version: 2,
      personId: 'neutral',
      interval: null,
      point: null,
      planned: 1,
      evaluated: 0,
      complete: false,
      spread: null,
      requiredStatus: 'unknown',
      judgeDisagreement: false,
      comparisonEligible: false,
    },
    trials: [],
    preparationWallTimeMs: 0,
    solutionWallTimeMs: 0,
    baselineDifference: null,
    artifacts: {},
  };
  const output: ExperimentResult = {
    schemaVersion: 4,
    scoringVersion: 2,
    status: 'incomplete',
    people: [],
    control,
    input: {
      task: 'Task',
      people: [
        { label: 'A', context: '', publicUrls: [] },
        { label: 'B', context: '', publicUrls: [] },
      ],
    },
    protocol: {
      version: 3,
      task: 'Task',
      repetitions: 1,
      participantOrder: [],
      rubric: { version: 2, criteria: [] },
      modelSettings: {},
      formatAttempts: 2,
      stageTimeoutMs: 600000,
      promptVersion: 'bsc-v2.4-camel-case',
      createdAt: '',
    },
    trials: [],
    comparison: { summary: 'Incomplete', difference: null, comparisonEligible: false },
    finishedAt: '',
    traces: [
      {
        id: 'control-person',
        person: 2,
        stage: 'person',
        label: 'Control',
        attempts: [{ index: 1, status: 'accepted', wallTimeMs: 1, error: null }],
        trace: { provider: 'codex', reasoningMs: 0, terminalMs: 0, summary: '' },
      },
    ],
  };
  expect(reportText(output, control, 'analysis')).toContain(
    'повторных запросов формата: неизвестно (старый формат)',
  );
});
