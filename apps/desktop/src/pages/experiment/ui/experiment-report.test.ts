import { expect, it } from 'vitest';
import type { ExperimentProtocol } from '@proteus-int/contracts';
import { formatInterval, formatScore, protocolText, traceMarker } from './experiment-report';

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
