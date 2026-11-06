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
    'reasoning-ms': 10,
    'terminal-ms': 0,
    summary: 'Запрос не завершён',
  };
  expect(
    traceMarker(
      {
        trace,
        attempts: [{ index: 1, status: 'failed', 'wall-time-ms': 10, error: 'CLI failed' }],
      },
      false,
    ),
  ).toBe('!');
  expect(
    traceMarker(
      {
        trace,
        attempts: [{ index: 1, status: 'format-rejected', 'wall-time-ms': 10, error: null }],
      },
      false,
    ),
  ).toBe('!');
  expect(
    traceMarker(
      { trace, attempts: [{ index: 1, status: 'accepted', 'wall-time-ms': 10, error: null }] },
      false,
    ),
  ).toBe('✓');
  expect(traceMarker({ trace: { ...trace, summary: '' }, attempts: [] }, true)).toBe('◇');
});
it('shows small positive criterion weights without rounding them to zero', () => {
  const protocol: ExperimentProtocol = {
    version: 2,
    task: 'Task',
    repetitions: 1,
    'participant-order': [['a', 'b', 'neutral']],
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
    'model-settings': {},
    'format-attempts': 2,
    'stage-timeout-ms': 600000,
    'prompt-version': 'bsc-v2.1',
    'created-at': '2026-11-06T00:00:00Z',
  };
  expect(protocolText(protocol)).toContain('Вес: 0.01;');
});
