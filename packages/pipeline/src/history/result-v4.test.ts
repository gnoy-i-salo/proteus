import { describe, expect, it } from 'vitest';
import { decodeV4Result } from './result-v4';

const clone = <T>(value: T): T => JSON.parse(JSON.stringify(value)) as T;

import earlyV4 from '../testing/legacy-results/v4-camel-no-execution.json';
import currentV4 from '../testing/legacy-results/v4-current-with-execution.json';
import v1 from '../testing/legacy-results/v1-unversioned-result.json';
import v2 from '../testing/legacy-results/v2-kebab-result.json';
import v3 from '../testing/legacy-results/v3-kebab-export.json';

describe('decodeV4Result', () => {
  it('keeps a bounded trace with more than 512 exposed reasoning blocks', () => {
    const value = decodeV4Result(currentV4);
    const reasoning = Array.from({ length: 600 }, (_, index) => ({
      id: `r${index}`,
      kind: 'summary',
      text: ` exact ${index}\r\n`,
    }));
    Object.assign(value.traces[0]!.trace, { reasoning });
    expect(decodeV4Result(value).traces[0]!.trace.reasoning).toEqual(reasoning);
  });
  it.each(['finishedAt', 'createdAt', 'retrievedAt', 'execution'])(
    'rejects impossible calendar dates in %s',
    (location) => {
      const value = clone(currentV4) as Record<string, unknown>;
      const impossible = '2026-02-30T10:24:36.000Z';
      if (location === 'finishedAt') value.finishedAt = impossible;
      if (location === 'createdAt')
        (value.protocol as Record<string, unknown>).createdAt = impossible;
      if (location === 'retrievedAt') {
        const report = (value.people as Record<string, unknown>[])[0]!.report as Record<
          string,
          unknown
        >;
        (report.evidence as Record<string, unknown>[])[0]!.retrievedAt = impossible;
      }
      if (location === 'execution') {
        const execution = value.execution as Record<string, unknown>;
        (execution.stages as Record<string, unknown>[])[0]!.startedAt = impossible;
      }
      expect(() => decodeV4Result(value)).toThrow();
    },
  );
  it('decodes the early v4 export exactly, keeping absent execution absent', () => {
    const decoded = decodeV4Result(earlyV4);
    expect(decoded).toEqual(earlyV4);
    expect('execution' in decoded).toBe(false);
  });

  it('decodes the current v4 export exactly, retaining execution and exportMetadata', () => {
    const decoded = decodeV4Result(currentV4);
    expect(decoded).toEqual(currentV4);
    expect(decoded.execution).toEqual((currentV4 as { execution: unknown }).execution);
    expect(decoded.exportMetadata?.version).toBe(1);
  });

  it('accepts the raw writer result shape without exportMetadata', () => {
    const raw = clone(currentV4) as Record<string, unknown>;
    delete raw.exportMetadata;
    const decoded = decodeV4Result(raw);
    expect(decoded).toEqual(raw);
    expect('exportMetadata' in decoded).toBe(false);
  });

  it.each([
    ['v1', v1],
    ['v2', v2],
    ['v3', v3],
  ])('rejects the %s envelope without converting it', (_era, file) => {
    expect(() => decodeV4Result(file)).toThrow(/schemaVersion/);
  });

  it.each([1, 2, 3, 5, '4', null])('rejects unsupported schemaVersion %j', (schemaVersion) => {
    expect(() =>
      decodeV4Result({ ...(clone(currentV4) as Record<string, unknown>), schemaVersion }),
    ).toThrow(/schemaVersion/);
  });

  it('rejects mixed legacy aliases on schema-owned objects', () => {
    const nested = clone(currentV4) as Record<string, unknown>;
    (nested.people as Record<string, unknown>[])[0]!['base-profile'] = {};
    expect(() => decodeV4Result(nested)).toThrow(/base-profile/);

    const trialAlias = clone(currentV4) as Record<string, unknown>;
    (trialAlias.trials as Record<string, unknown>[])[0]!['person-id'] = 'a';
    expect(() => decodeV4Result(trialAlias)).toThrow(/person-id/);

    const topLevel = clone(currentV4) as Record<string, unknown>;
    topLevel['finished-at'] = '2026-11-08T10:24:36.000Z';
    expect(() => decodeV4Result(topLevel)).toThrow(/finished-at/);

    const exportAlias = clone(currentV4) as Record<string, unknown>;
    exportAlias['export-metadata'] = {};
    expect(() => decodeV4Result(exportAlias)).toThrow(/export-metadata/);
  });

  it.each([
    ['null', null],
    ['array', []],
    ['string', '{}'],
  ])('rejects a non-object envelope: %s', (_name, value) => {
    expect(() => decodeV4Result(value)).toThrow();
  });

  it('rejects corrupt types, unsafe numbers and malformed timestamps', () => {
    const badTask = clone(currentV4) as Record<string, unknown>;
    (badTask.input as Record<string, unknown>).task = 42;
    expect(() => decodeV4Result(badTask)).toThrow(/task/);

    const badStatus = clone(currentV4) as Record<string, unknown>;
    badStatus.status = 'unknown';
    expect(() => decodeV4Result(badStatus)).toThrow(/status/);

    const badTimestamp = clone(currentV4) as Record<string, unknown>;
    badTimestamp.finishedAt = '2026-11-08 10:24:36';
    expect(() => decodeV4Result(badTimestamp)).toThrow(/finishedAt/);

    const inverted = clone(currentV4) as Record<string, unknown>;
    const score = (inverted.people as Record<string, unknown>[])[0]!.score as Record<
      string,
      unknown
    >;
    score.interval = { lower: 80, upper: 20 };
    expect(() => decodeV4Result(inverted)).toThrow(/interval/);

    const infinite = clone(currentV4) as Record<string, unknown>;
    (infinite.people as Record<string, unknown>[])[0]!.preparationWallTimeMs =
      Number.POSITIVE_INFINITY;
    expect(() => decodeV4Result(infinite)).toThrow(/WallTime/);

    const badVerification = clone(currentV4) as Record<string, unknown>;
    const evidence = (
      (badVerification.people as Record<string, unknown>[])[0]!.report as Record<string, unknown>
    ).evidence as Record<string, unknown>[];
    evidence[0]!.verification = 'verified';
    expect(() => decodeV4Result(badVerification)).toThrow(/verification/);

    const badExecution = clone(currentV4) as Record<string, unknown>;
    badExecution.execution = { schemaVersion: 2, runId: 'run-x', stages: [] };
    expect(() => decodeV4Result(badExecution)).toThrow();

    const nullExecution = clone(currentV4) as Record<string, unknown>;
    nullExecution.execution = null;
    expect(() => decodeV4Result(nullExecution)).toThrow();
  });

  it('returns detached objects and never mutates the input', () => {
    const input = clone(currentV4);
    const before = JSON.stringify(input);
    const decoded = decodeV4Result(input);
    expect(JSON.stringify(input)).toBe(before);

    const mutableSkills = decoded.people[0]!.profile.skills as string[];
    mutableSkills.push('injected');
    const artifacts = decoded.people[0]!.artifacts as Record<string, string>;
    artifacts.injected = 'value';
    const again = decodeV4Result(input);
    expect(again.people[0]!.profile.skills).not.toContain('injected');
    expect(Object.keys(again.people[0]!.artifacts)).not.toContain('injected');
    expect(again).toEqual(JSON.parse(before));
  });

  it('preserves exact Unicode, CRLF and opaque artifact or map keys', () => {
    const input = clone(currentV4) as Record<string, unknown>;
    const task = '  «Точный» текст\r\nс NUL:\u0000 и emoji 😀  ';
    (input.input as Record<string, unknown>).task = task;
    (input.people as Record<string, unknown>[])[0]!.artifacts = Object.fromEntries([
      ['__proto__', 'proto-value'],
      ['constructor', 'ctor-value'],
      ['prototype', 'proto2-value'],
      ['.agents/skills/persona-1/SKILL.md', 'file-value'],
    ]);
    const decoded = decodeV4Result(input);
    expect(decoded.input.task).toBe(task);
    const artifacts = decoded.people[0]!.artifacts;
    expect(artifacts['__proto__']).toBe('proto-value');
    expect(artifacts.constructor).toBe('ctor-value');
    expect(artifacts.prototype).toBe('proto2-value');
    expect(artifacts['.agents/skills/persona-1/SKILL.md']).toBe('file-value');
    const globalCheck: Record<string, unknown> = {};
    expect(globalCheck.polluted).toBeUndefined();
  });

  it('preserves redaction metadata and historical BSC values unchanged', () => {
    const redacted = clone(currentV4) as Record<string, unknown>;
    redacted.exportMetadata = {
      version: 1,
      redacted: true,
      omittedStyleExemplars: 2,
      omittedSourceExcerpts: 1,
      preparationReasoningOmitted: true,
      answerTextMayBeRedacted: true,
      scoresReferenceOriginalAnswers: true,
      limitations: ['Connected-source excerpts omitted.', 'Scores describe original answers.'],
    };
    const decoded = decodeV4Result(redacted);
    expect(decoded.exportMetadata).toEqual(redacted.exportMetadata);
    expect(decoded.people[0]!.score.point).toBe(100);
    expect(decoded.people[1]!.score.interval).toEqual({ lower: 75, upper: 75 });
    expect(decoded.comparison.difference).toEqual({ lower: 25, upper: 25 });
  });

  it('rejects envelopes above the 8 MiB native save bound', () => {
    const huge = clone(currentV4) as Record<string, unknown>;
    (huge.input as Record<string, unknown>).task = 'x'.repeat(9 * 1024 * 1024);
    expect(() => decodeV4Result(huge)).toThrow(/8 MiB/);
  });
});
