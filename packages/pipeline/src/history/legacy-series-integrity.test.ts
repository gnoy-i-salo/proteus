import { expect, it } from 'vitest';
import v2 from '../testing/legacy-results/v2-kebab-result.json';
import v3 from '../testing/legacy-results/v3-kebab-export.json';
import { decodeLegacySeriesResult } from './legacy-series';

it('preserves recorded historical conclusions and numbers without rerunning score formulas', () => {
  const raw = structuredClone(v2);
  raw.comparison.summary = '\uFEFFИсторическая сводка\r\n\u0000';
  raw.comparison.difference = { lower: 20, upper: 30 };
  raw.people[0]!['baseline-difference'] = { lower: 19, upper: 31 };
  // Older observations can contain discrepancies: the reader validates shape,
  // not the historical scorer. Copied scores must still be the same record.
  raw.trials[0]!.score.criteria[0]!.interval = { lower: 0.7, upper: 0.9 };
  raw.people[0]!.trials[0]!.score = structuredClone(raw.trials[0]!.score);
  const result = decodeLegacySeriesResult(raw);
  expect(result.comparison.summary).toBe(raw.comparison.summary);
  expect(result.comparison.difference).toEqual(raw.comparison.difference);
  expect(result.people[0].baselineDifference).toEqual(raw.people[0]!['baseline-difference']);
  expect(result.trials[0]!.score!.criteria[0]!.interval).toEqual({ lower: 0.7, upper: 0.9 });
});

for (const status of ['complete', 'incomplete', 'cancelled']) {
  it(`rejects conflicting copied reply details in a ${status} result`, () => {
    const raw = structuredClone(v3);
    raw.status = status;
    Object.assign(
      raw.people[0]!.trials[0]!.attempt.reply,
      Object.fromEntries<unknown>([['style-conflicts', ['changed copy']]]),
    );
    expect(() => decodeLegacySeriesResult(raw)).toThrow(/copied|conflict/i);
  });
  if (status !== 'complete')
    it(`rejects conflicting copied trial bodies in a ${status} result`, () => {
      const raw = structuredClone(v2);
      raw.status = status;
      raw.comparison.summary =
        'Серия не завершена. Баллы относятся только к оценённым ответам; итоговое сравнение недоступно.';
      raw.people[0]!.trials[0]!.attempt.answer = 'Substituted trial copy';
      expect(() => decodeLegacySeriesResult(raw)).toThrow(/copied|conflict/i);
    });
}
it('retains writer-emitted reasoning collections within the overall file budget', () => {
  const raw = structuredClone(v2);
  raw.traces[0]!.trace.reasoning = Array.from({ length: 600 }, (_, index) => ({
    id: `1:${index}`,
    kind: 'summary',
    text: `Exposed summary ${index}\r\n`,
  }));
  expect(decodeLegacySeriesResult(raw).traces[0]!.trace.reasoning).toEqual(
    raw.traces[0]!.trace.reasoning,
  );
});
it('does not impose a new URL length cap on historical input', () => {
  const raw = structuredClone(v2);
  raw.input.people[0]!['public-urls'] = ['https://sample.example/' + 'x'.repeat(5000)];
  expect(decodeLegacySeriesResult(raw).input.people[0].publicUrls).toEqual(
    raw.input.people[0]!['public-urls'],
  );
});

for (const status of ['answered', 'failed'])
  it(`rejects a ${status} trial that still claims a completed score even when copies match`, () => {
    const raw = structuredClone(v2);
    raw.status = 'incomplete';
    const trial = raw.trials[0]!;
    Object.assign(trial, {
      status,
      error: 'Interrupted',
      evaluations: [],
      ...(status === 'failed' ? { attempt: null } : {}),
    });
    raw.people[0]!.trials[0] = structuredClone(trial);
    raw.people[0]!.attempt = structuredClone(trial.attempt);
    expect(() => decodeLegacySeriesResult(raw)).toThrow();
  });

it('preserves absence of the optional legacy source snapshot extension', () => {
  const raw = structuredClone(v3);
  Reflect.deleteProperty(raw.protocol, 'source-snapshots');
  expect(decodeLegacySeriesResult(raw).protocol).not.toHaveProperty('sourceSnapshots');
});

it('retains a CLI-exposed 48 KiB reasoning block instead of applying model JSON field limits', () => {
  const raw = structuredClone(v2);
  raw.traces[0]!.trace.reasoning = [
    { id: '1:' + 'x'.repeat(160), kind: 'summary', text: 'x'.repeat(48 * 1024) },
  ];
  expect(decodeLegacySeriesResult(raw).traces[0]!.trace.reasoning).toEqual(
    raw.traces[0]!.trace.reasoning,
  );
});
it('retains more than eight calls from source collection and analysis in one legacy stage', () => {
  const raw = structuredClone(v3);
  raw.traces[1]!.attempts = Array.from({ length: 9 }, (_, index) => ({
    ...raw.traces[1]!.attempts[0]!,
    index: index + 1,
  }));
  raw.traces[1]!.trace['wall-time-ms'] *= 9;
  raw.traces[1]!.trace['reasoning-ms'] *= 9;
  raw.people[0]!['preparation-wall-time-ms'] += 60000 * 8;
  expect(decodeLegacySeriesResult(raw).traces[1]!.attempts).toHaveLength(9);
});
