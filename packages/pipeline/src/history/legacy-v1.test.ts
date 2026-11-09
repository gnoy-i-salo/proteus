import { describe, expect, it } from 'vitest';
import v2Fixture from '../testing/legacy-results/v2-kebab-result.json';
import v3Fixture from '../testing/legacy-results/v3-kebab-export.json';
import v4CurrentFixture from '../testing/legacy-results/v4-current-with-execution.json';
import v4EarlyFixture from '../testing/legacy-results/v4-camel-no-execution.json';
import v1Fixture from '../testing/legacy-results/v1-unversioned-result.json';
import { decodeLegacyV1Result } from './legacy-v1';

const agentsMd = 'AGENTS.md';
const personaSkillMd = '.agents/skills/persona-1/SKILL.md';

const expectedFixtureResult = {
  input: {
    task: 'Design a public reading room',
    people: [
      {
        label: 'Ada Sample',
        context: 'Synthetic fixture persona: careful planner.',
        publicUrls: ['https://sample.example/ada-notes'],
      },
      {
        label: 'Boris Fixture',
        context: 'Synthetic fixture persona: rapid improviser.',
        publicUrls: [],
      },
    ],
  },
  people: [
    {
      report: {
        evidence: [
          {
            id: 'e1',
            sourceUrl: 'https://sample.example/ada-notes',
            kind: 'fact',
            summary: 'Plans each milestone before acting',
          },
        ],
        estimatedBsc: 15,
        effort: 'medium',
        character: 'Deliberate',
        thinkingStyle: 'Structured',
        activity: 'Planning',
        behaviorSummary: 'Synthetic behavior summary for fixture persona A',
      },
      profile: {
        id: 'a',
        label: 'Ada Sample',
        palette: 'turquoise',
        archetype: 'Careful planner (synthetic)',
        skills: ['Plan in milestones'],
      },
      attempt: {
        answer: 'Answer A: phased plan with milestones.',
        trace: {
          provider: 'codex',
          reasoningMs: 90000,
          terminalMs: 0,
          summary: 'Completed synthetic run',
        },
      },
      score: { personId: 'a', reasoningMinutes: 1.5, bsc: 0.1 },
      artifacts: {
        [agentsMd]:
          '# Simulated persona\n\nCareful planner (synthetic)\n\nUse the attached skills to approach the task. This is a behavioral simulation, not the real person.\n',
        [personaSkillMd]:
          "---\nname: persona-1\ndescription: Behavioral strategy for this persona's task\n---\n\nPlan in milestones\n",
      },
    },
    {
      report: {
        evidence: [
          {
            id: 'e1',
            sourceUrl: '',
            kind: 'hypothesis',
            summary: 'Tests ideas quickly',
          },
        ],
        estimatedBsc: 12,
        effort: 'medium',
        character: 'Quick',
        thinkingStyle: 'Exploratory',
        activity: 'Prototyping',
        behaviorSummary: 'Synthetic behavior summary for fixture persona B',
      },
      profile: {
        id: 'b',
        label: 'Boris Fixture',
        palette: 'violet',
        archetype: 'Rapid improviser (synthetic)',
        skills: ['Prototype early'],
      },
      attempt: {
        answer: 'Answer B: quick prototype sketch.',
        trace: {
          provider: 'codex',
          reasoningMs: 60000,
          terminalMs: 0,
          summary: 'Completed synthetic run',
        },
      },
      score: { personId: 'b', reasoningMinutes: 1, bsc: 0.08333333333333333 },
      artifacts: {
        [agentsMd]:
          '# Simulated persona\n\nRapid improviser (synthetic)\n\nUse the attached skills to approach the task. This is a behavioral simulation, not the real person.\n',
        [personaSkillMd]:
          "---\nname: persona-1\ndescription: Behavioral strategy for this persona's task\n---\n\nPrototype early\n",
      },
    },
  ],
  comparison: {
    summary: 'Synthetic comparative summary: planning depth differs between fixtures.',
    scores: [
      { personId: 'a', reasoningMinutes: 1.5, bsc: 0.1 },
      { personId: 'b', reasoningMinutes: 1, bsc: 0.08333333333333333 },
    ],
    keyDifferences: ['Fixture A plans in phases; fixture B prototypes first.'],
    people: [
      { strengths: ['Structure'], weaknesses: [], observations: ['Plans before acting'] },
      { strengths: ['Speed'], weaknesses: [], observations: ['Tests ideas quickly'] },
    ],
    ratio: 1.2000000000000002,
  },
  finishedAt: '2026-11-08T10:24:36.000Z',
};

function mutableFixture(): Record<string, unknown> {
  return JSON.parse(JSON.stringify(v1Fixture)) as Record<string, unknown>;
}

function node(value: Record<string, unknown>, key: string): Record<string, unknown> {
  return value[key] as Record<string, unknown>;
}

function at(value: unknown, index: number): Record<string, unknown> {
  return (value as Record<string, unknown>[])[index] as Record<string, unknown>;
}

function personOf(value: Record<string, unknown>, index: number): Record<string, unknown> {
  return at(node(value, 'people'), index);
}

function expectRejected(mutate: (value: Record<string, unknown>) => unknown): void {
  const value = mutableFixture();
  const replacement = mutate(value);
  expect(() => decodeLegacyV1Result(replacement === undefined ? value : replacement)).toThrow();
}

describe('legacy v1 saved-result decoder', () => {
  it('maps the pinned v1 fixture to era field names with unchanged BSC values', () => {
    const decoded = decodeLegacyV1Result(v1Fixture);
    expect(decoded).toEqual(expectedFixtureResult);
    expect(decoded.people[0]?.score.bsc).toBe(0.1);
    expect(decoded.people[1]?.score.bsc).toBe(0.08333333333333333);
    expect(decoded.comparison.scores[0]?.bsc).toBe(0.1);
    expect(decoded.comparison.ratio).toBe(1.2000000000000002);
    expect(decoded.people[1]?.report.evidence[0]?.sourceUrl).toBe('');
    expect(decoded.people[0]?.attempt.trace.reasoningMs).toBe(90000);
    expect(Object.keys(decoded.people[0]?.artifacts ?? {})).toEqual([agentsMd, personaSkillMd]);
  });

  it('preserves Cyrillic, decomposed Unicode, CRLF and BOM inside strings exactly', () => {
    const value = mutableFixture();
    const task =
      '\uFEFFЗадача: «читальный зал» для Е\u0308лкиной\r\nВторая строка оканчивается тут';
    const context = '  контекст с краевыми пробелами и ё \r\nдвумя строками  ';
    const answer = 'Ответ с кириллицей: е\u0308лка.\r\n\r\nНовый абзац после CRLF-пары.';
    const summary = 'Сводка с combining mark: e\u0308 и \uFEFF внутри текста.';
    const artifact = 'line one\r\nline two\nline three\uFEFF';
    node(value, 'input')['task'] = task;
    at(node(value, 'input')['people'], 0)['context'] = context;
    const personA = personOf(value, 0);
    node(personA, 'attempt')['answer'] = answer;
    at(node(personA, 'report')['evidence'], 0)['summary'] = summary;
    node(personA, 'artifacts')[agentsMd] = artifact;
    const decoded = decodeLegacyV1Result(value);
    expect(decoded.input.task).toBe(task);
    expect(decoded.input.people[0]?.context).toBe(context);
    expect(decoded.people[0]?.attempt.answer).toBe(answer);
    expect(decoded.people[0]?.report.evidence[0]?.summary).toBe(summary);
    expect(decoded.people[0]?.artifacts[agentsMd]).toBe(artifact);
  });

  it('returns detached data and never mutates the parsed input', () => {
    const value = mutableFixture();
    const before = JSON.stringify(value);
    const decoded = decodeLegacyV1Result(value);
    expect(JSON.stringify(value)).toBe(before);
    expect(decoded).not.toBe(value);
    expect(decoded.input).not.toBe(node(value, 'input'));
    expect(decoded.people).not.toBe(value['people']);
    const mutated = JSON.parse(JSON.stringify(decoded)) as Record<string, unknown>;
    node(mutated, 'input')['task'] = 'changed afterwards';
    at(node(mutated, 'people'), 0)['score'] = null;
    expect(decoded.input.task).toBe('Design a public reading room');
    expect(decoded.people[0]?.score.bsc).toBe(0.1);
    expect(decodeLegacyV1Result(value)).toEqual(decoded);
  });

  it('exposes era fields only, without fabricated current fields', () => {
    const decoded = decodeLegacyV1Result(v1Fixture);
    const resultFields = [
      'schemaVersion',
      'scoringVersion',
      'protocol',
      'status',
      'control',
      'trials',
      'traces',
      'execution',
      'exportMetadata',
    ];
    for (const key of resultFields) expect(decoded).not.toHaveProperty(key);
    const person = decoded.people[0];
    expect(person).not.toHaveProperty('baseProfile');
    expect(person).not.toHaveProperty('trials');
    expect(person?.profile).not.toHaveProperty('rules');
    expect(decoded.comparison).not.toHaveProperty('comparisonEligible');
    expect(decoded.comparison).not.toHaveProperty('difference');
  });

  it.each([
    ['pinned v2 kebab result fixture', () => v2Fixture],
    ['pinned v3 kebab export fixture', () => v3Fixture],
    ['pinned early v4 camel fixture', () => v4EarlyFixture],
    ['pinned current v4 fixture', () => v4CurrentFixture],
  ] as const)('rejects the %s as a non-v1 envelope', (_name, load) => {
    expect(() => decodeLegacyV1Result(load())).toThrow();
  });

  it.each([
    [
      'schema-version 2 masquerading as v1',
      (v: Record<string, unknown>): void => void (v['schema-version'] = 2),
    ],
    [
      'current schemaVersion 4',
      (v: Record<string, unknown>): void => void (v['schemaVersion'] = 4),
    ],
    ['scoringVersion field', (v: Record<string, unknown>): void => void (v['scoringVersion'] = 2)],
    ['protocol field', (v: Record<string, unknown>): void => void (v['protocol'] = {})],
    ['status field', (v: Record<string, unknown>): void => void (v['status'] = 'complete')],
    ['control field', (v: Record<string, unknown>): void => void (v['control'] = {})],
    ['trials field', (v: Record<string, unknown>): void => void (v['trials'] = [])],
    ['traces field', (v: Record<string, unknown>): void => void (v['traces'] = [])],
    ['execution field', (v: Record<string, unknown>): void => void (v['execution'] = {})],
    ['unknown extra field', (v: Record<string, unknown>): void => void (v['export-metadata'] = {})],
    [
      'camel finishedAt alias',
      (v: Record<string, unknown>): void =>
        void (v['finishedAt'] = expectedFixtureResult.finishedAt),
    ],
    [
      'finished-at plus finishedAt',
      (v: Record<string, unknown>): void => void (v['finishedAt'] = 'x'),
    ],
    ['missing comparison', (v: Record<string, unknown>): void => void delete v['comparison']],
    ['missing finished-at', (v: Record<string, unknown>): void => void delete v['finished-at']],
  ] as const)('rejects %s at the result envelope', (_name, mutate) => {
    expectRejected(mutate);
  });

  it.each([
    [
      'report with estimated-bsc plus camel estimatedBsc',
      (v: Record<string, unknown>): void =>
        void (node(personOf(v, 0), 'report')['estimatedBsc'] = 15),
    ],
    [
      'report with only camel estimatedBsc',
      (v: Record<string, unknown>): void => {
        const report = node(personOf(v, 0), 'report');
        void delete report['estimated-bsc'];
        report['estimatedBsc'] = 15;
      },
    ],
    [
      'evidence with source-url plus camel sourceUrl',
      (v: Record<string, unknown>): void => {
        const evidence = at(node(personOf(v, 0), 'report')['evidence'], 0);
        evidence['sourceUrl'] = 'https://sample.example/other';
      },
    ],
    [
      'score with person-id plus camel personId',
      (v: Record<string, unknown>): void => void (node(personOf(v, 1), 'score')['personId'] = 'b'),
    ],
    [
      'trace with reasoning-ms plus camel reasoningMs',
      (v: Record<string, unknown>): void => {
        const trace = node(node(personOf(v, 0), 'attempt'), 'trace');
        trace['reasoningMs'] = 90000;
      },
    ],
    [
      'comparison with key-differences plus camel keyDifferences',
      (v: Record<string, unknown>): void => void (node(v, 'comparison')['keyDifferences'] = []),
    ],
    [
      'person with current baseProfile and trials fields',
      (v: Record<string, unknown>): void => {
        const person = personOf(v, 0);
        person['baseProfile'] = {};
        person['trials'] = [];
      },
    ],
    [
      'profile with current rules field',
      (v: Record<string, unknown>): void => void (node(personOf(v, 0), 'profile')['rules'] = []),
    ],
    [
      'comparison with current comparisonEligible field',
      (v: Record<string, unknown>): void =>
        void (node(v, 'comparison')['comparisonEligible'] = true),
    ],
    [
      'report with unknown extra field',
      (v: Record<string, unknown>): void =>
        void (node(personOf(v, 0), 'report')['sourceStatus'] = 'synthetic'),
    ],
  ] as const)('rejects mixed or current-era keys: %s', (_name, mutate) => {
    expectRejected(mutate);
  });

  it.each([
    ['null result', () => null],
    ['array result', () => []],
    ['string result', () => 'legacy'],
    ['number result', () => 4],
    ['null input', (v: Record<string, unknown>): void => void (v['input'] = null)],
    ['null comparison', (v: Record<string, unknown>): void => void (v['comparison'] = null)],
    ['people as object', (v: Record<string, unknown>): void => void (v['people'] = {})],
    ['single person', (v: Record<string, unknown>): void => void (v['people'] = [personOf(v, 0)])],
    [
      'three people',
      (v: Record<string, unknown>): void => {
        const people = v['people'] as Record<string, unknown>[];
        v['people'] = [...people, personOf(v, 0)];
      },
    ],
    [
      'null person entry',
      (v: Record<string, unknown>): void => void (v['people'] = [null, personOf(v, 1)]),
    ],
    [
      'array person entry',
      (v: Record<string, unknown>): void => void (v['people'] = [[], personOf(v, 1)]),
    ],
    [
      'report missing evidence',
      (v: Record<string, unknown>): void => void delete node(personOf(v, 0), 'report')['evidence'],
    ],
    [
      'null evidence item',
      (v: Record<string, unknown>): void =>
        void (node(personOf(v, 0), 'report')['evidence'] = [null]),
    ],
    [
      'evidence with unknown kind',
      (v: Record<string, unknown>): void => {
        const evidence = at(node(personOf(v, 0), 'report')['evidence'], 0);
        evidence['kind'] = 'opinion';
      },
    ],
    [
      'report with numeric effort',
      (v: Record<string, unknown>): void => void (node(personOf(v, 0), 'report')['effort'] = 2),
    ],
    [
      'profile with unknown palette',
      (v: Record<string, unknown>): void =>
        void (node(personOf(v, 1), 'profile')['palette'] = 'amber'),
    ],
    [
      'attempt missing trace',
      (v: Record<string, unknown>): void => void delete node(personOf(v, 0), 'attempt')['trace'],
    ],
    [
      'trace with numeric summary',
      (v: Record<string, unknown>): void =>
        void (node(node(personOf(v, 0), 'attempt'), 'trace')['summary'] = 5),
    ],
    [
      'score missing bsc',
      (v: Record<string, unknown>): void => void delete node(personOf(v, 0), 'score')['bsc'],
    ],
    [
      'artifacts as array',
      (v: Record<string, unknown>): void => void (personOf(v, 0)['artifacts'] = [agentsMd]),
    ],
    [
      'artifact content as number',
      (v: Record<string, unknown>): void => void (node(personOf(v, 0), 'artifacts')[agentsMd] = 5),
    ],
    [
      'comparison with single score',
      (v: Record<string, unknown>): void =>
        void (node(v, 'comparison')['scores'] = [at(node(v, 'comparison')['scores'], 0)]),
    ],
    [
      'comparison people as object',
      (v: Record<string, unknown>): void => void (node(v, 'comparison')['people'] = {}),
    ],
    [
      'observation missing weaknesses',
      (v: Record<string, unknown>): void =>
        void delete at(node(v, 'comparison')['people'], 0)['weaknesses'],
    ],
    [
      'string ratio',
      (v: Record<string, unknown>): void => void (node(v, 'comparison')['ratio'] = 'n/a'),
    ],
  ] as const)('rejects corrupt, null or malformed data: %s', (_name, mutate) => {
    expectRejected(mutate);
  });

  it.each([
    [
      'oversize task',
      (v: Record<string, unknown>): void => void (node(v, 'input')['task'] = 'a'.repeat(4001)),
    ],
    ['blank task', (v: Record<string, unknown>): void => void (node(v, 'input')['task'] = '   ')],
    [
      'oversize label',
      (v: Record<string, unknown>): void =>
        void (at(node(v, 'input')['people'], 0)['label'] = 'a'.repeat(81)),
    ],
    [
      'blank label',
      (v: Record<string, unknown>): void =>
        void (at(node(v, 'input')['people'], 0)['label'] = ' \t'),
    ],
    [
      'oversize context',
      (v: Record<string, unknown>): void =>
        void (at(node(v, 'input')['people'], 0)['context'] = 'a'.repeat(4001)),
    ],
    [
      'six public urls',
      (v: Record<string, unknown>): void => {
        const urls = at(node(v, 'input')['people'], 0)['public-urls'] as unknown[];
        for (let index = 0; index < 5; index += 1)
          urls.push(`https://sample.example/extra-${index}`);
      },
    ],
    [
      '31 evidence items',
      (v: Record<string, unknown>): void => {
        const report = node(personOf(v, 0), 'report');
        const evidence = at(report['evidence'], 0);
        report['evidence'] = Array.from({ length: 31 }, () => ({ ...evidence }));
      },
    ],
    [
      '31 skills',
      (v: Record<string, unknown>): void =>
        void (node(personOf(v, 0), 'profile')['skills'] = Array.from(
          { length: 31 },
          () => 'Plan in milestones',
        )),
    ],
    [
      '31 key differences',
      (v: Record<string, unknown>): void =>
        void (node(v, 'comparison')['key-differences'] = Array.from(
          { length: 31 },
          () => 'difference',
        )),
    ],
    [
      'oversize answer',
      (v: Record<string, unknown>): void =>
        void (node(personOf(v, 0), 'attempt')['answer'] = 'a'.repeat(24001)),
    ],
    [
      'oversize trace summary',
      (v: Record<string, unknown>): void => {
        node(node(personOf(v, 0), 'attempt'), 'trace')['summary'] = 'a'.repeat(24001);
      },
    ],
    [
      '257 artifact entries',
      (v: Record<string, unknown>): void => {
        const artifacts = node(personOf(v, 0), 'artifacts');
        for (let index = 0; index < 255; index += 1) artifacts[`file-${index}.md`] = 'content';
      },
    ],
    [
      'oversize artifact key',
      (v: Record<string, unknown>): void => {
        node(personOf(v, 0), 'artifacts')['x'.repeat(513)] = 'content';
      },
    ],
  ] as const)('rejects oversize or blank fields: %s', (_name, mutate) => {
    expectRejected(mutate);
  });

  it.each([
    [
      'bsc NaN',
      (v: Record<string, unknown>): void =>
        void (node(personOf(v, 0), 'score')['bsc'] = Number.NaN),
    ],
    [
      'bsc Infinity',
      (v: Record<string, unknown>): void =>
        void (node(personOf(v, 0), 'score')['bsc'] = Number.POSITIVE_INFINITY),
    ],
    [
      'negative bsc',
      (v: Record<string, unknown>): void => void (node(personOf(v, 0), 'score')['bsc'] = -0.1),
    ],
    [
      'string bsc',
      (v: Record<string, unknown>): void => void (node(personOf(v, 0), 'score')['bsc'] = '0.1'),
    ],
    [
      'negative reasoning-minutes',
      (v: Record<string, unknown>): void =>
        void (node(personOf(v, 0), 'score')['reasoning-minutes'] = -0.5),
    ],
    [
      'negative reasoning-ms',
      (v: Record<string, unknown>): void => {
        node(node(personOf(v, 0), 'attempt'), 'trace')['reasoning-ms'] = -1;
      },
    ],
    [
      'string reasoning-ms',
      (v: Record<string, unknown>): void => {
        node(node(personOf(v, 0), 'attempt'), 'trace')['reasoning-ms'] = '90000';
      },
    ],
    [
      'negative terminal-ms',
      (v: Record<string, unknown>): void => {
        node(node(personOf(v, 0), 'attempt'), 'trace')['terminal-ms'] = -0.5;
      },
    ],
    [
      'zero estimated-bsc',
      (v: Record<string, unknown>): void =>
        void (node(personOf(v, 0), 'report')['estimated-bsc'] = 0),
    ],
    [
      'negative estimated-bsc',
      (v: Record<string, unknown>): void =>
        void (node(personOf(v, 0), 'report')['estimated-bsc'] = -3),
    ],
    [
      'string estimated-bsc',
      (v: Record<string, unknown>): void =>
        void (node(personOf(v, 0), 'report')['estimated-bsc'] = '15'),
    ],
    [
      'fractional ratio',
      (v: Record<string, unknown>): void => void (node(v, 'comparison')['ratio'] = 0.5),
    ],
    [
      'negative ratio',
      (v: Record<string, unknown>): void => void (node(v, 'comparison')['ratio'] = -2),
    ],
    [
      'string ratio',
      (v: Record<string, unknown>): void => void (node(v, 'comparison')['ratio'] = '2'),
    ],
  ] as const)('rejects invalid numerics: %s', (_name, mutate) => {
    expectRejected(mutate);
  });

  it.each([
    [
      'date-only finished-at',
      (v: Record<string, unknown>): void => void (v['finished-at'] = '2026-11-08'),
    ],
    [
      'space-separated finished-at',
      (v: Record<string, unknown>): void => void (v['finished-at'] = '2026-11-08 10:24:36.000Z'),
    ],
    [
      'offset finished-at',
      (v: Record<string, unknown>): void =>
        void (v['finished-at'] = '2026-11-08T13:24:36.000+03:00'),
    ],
    [
      'invalid calendar finished-at',
      (v: Record<string, unknown>): void => void (v['finished-at'] = '2026-13-08T10:24:36.000Z'),
    ],
    [
      'overflowing day finished-at',
      (v: Record<string, unknown>): void => void (v['finished-at'] = '2026-02-30T10:24:36.000Z'),
    ],
    ['null finished-at', (v: Record<string, unknown>): void => void (v['finished-at'] = null)],
    [
      'numeric finished-at',
      (v: Record<string, unknown>): void => void (v['finished-at'] = 1760000000000),
    ],
  ] as const)('rejects invalid timestamps: %s', (_name, mutate) => {
    expectRejected(mutate);
  });

  it('accepts a finite ratio at or above one and keeps zero durations', () => {
    const value = mutableFixture();
    node(value, 'comparison')['ratio'] = 2;
    node(node(personOf(value, 0), 'attempt'), 'trace')['reasoning-ms'] = 0;
    node(personOf(value, 0), 'score')['reasoning-minutes'] = 0;
    const decoded = decodeLegacyV1Result(value);
    expect(decoded.comparison.ratio).toBe(2);
    expect(decoded.people[0]?.attempt.trace.reasoningMs).toBe(0);
    expect(decoded.people[0]?.score.reasoningMinutes).toBe(0);
  });

  it('keeps a recorded null ratio without deriving a new comparison from saved scores', () => {
    const value = mutableFixture();
    node(value, 'comparison')['ratio'] = null;
    expect(decodeLegacyV1Result(value).comparison.ratio).toBeNull();
  });

  it('preserves opaque artifact keys as inert own data properties', () => {
    const safe = mutableFixture();
    const safeArtifacts = JSON.parse(
      '{"__proto__": "opaque prototype name", "constructor": "opaque constructor name", "prototype": "opaque prototype file"}',
    ) as Record<string, unknown>;
    Object.assign(safeArtifacts, node(personOf(safe, 0), 'artifacts'));
    personOf(safe, 0)['artifacts'] = safeArtifacts;
    safeArtifacts['CONSTRUCTOR.md'] = 'opaque key stays';
    safeArtifacts['artifact key with spaces.md'] = 'spaces stay';
    const decoded = decodeLegacyV1Result(safe);
    expect(decoded.people[0]?.artifacts['CONSTRUCTOR.md']).toBe('opaque key stays');
    expect(decoded.people[0]?.artifacts['artifact key with spaces.md']).toBe('spaces stay');
    expect(decoded.people[0]?.artifacts[agentsMd]).toContain('Simulated persona');
    expect(decoded.people[0]?.artifacts).toEqual(safeArtifacts);
    expect(Object.hasOwn(decoded.people[0].artifacts, '__proto__')).toBe(true);
    expect(Object.getPrototypeOf(decoded.people[0].artifacts)).toBe(Object.prototype);
    expect(Object.prototype).not.toHaveProperty('polluted');
  });

  it('preserves long source URLs accepted by the actual era writer', () => {
    const value = mutableFixture();
    const url = `https://sample.example/${'a'.repeat(5000)}?exact=e\u0301`;
    const input = at(node(value, 'input')['people'], 0);
    input['public-urls'] = [url];
    const evidence = at(node(personOf(value, 0), 'report')['evidence'], 0);
    evidence['source-url'] = url;
    const decoded = decodeLegacyV1Result(value);
    expect(decoded.input.people[0].publicUrls).toEqual([url]);
    expect(decoded.people[0].report.evidence[0]?.sourceUrl).toBe(url);
  });

  it('rejects a result over the 8 MiB file envelope', () => {
    const value = mutableFixture();
    node(personOf(value, 0), 'artifacts')[agentsMd] = 'x'.repeat(9 * 1024 * 1024);
    expect(() => decodeLegacyV1Result(value)).toThrow(/8 MiB/u);
  });
});
