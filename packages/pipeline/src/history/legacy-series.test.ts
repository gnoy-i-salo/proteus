import { describe, expect, it } from 'vitest';
import v2Fixture from '../testing/legacy-results/v2-kebab-result.json';
import v3Fixture from '../testing/legacy-results/v3-kebab-export.json';
import { decodeLegacySeriesResult } from './legacy-series';

/**
 * Reader tests for the exact legacy series-result schemas:
 *  - schema-version 2: raw save, prompt bsc-v2.1, no style/source fields, no export-metadata;
 *  - schema-version 3: raw save and redacted export, prompts bsc-v2.2-source-style /
 *    bsc-v2.3-source-identity, optional style/source fields, optional reply v1,
 *    optional export-metadata v1.
 * Both eras share scoring-version 2. Recorded BSC values are preserved verbatim and are
 * never recomputed by these tests.
 */

const traceHigh = {
  provider: 'codex',
  reasoningMs: 60000,
  wallTimeMs: 60000,
  terminalMs: 0,
  summary: 'Completed synthetic stage',
  reasoning: [],
  requestedModel: '',
  requestedEffort: 'high',
  resolvedModel: null,
  appliedEffort: null,
  cliVersion: null,
  activeReasoningMs: null,
  reasoningTruncated: false,
};

const traceMedium = { ...traceHigh, requestedEffort: 'medium' };

const traceRedactedPrep = {
  ...traceMedium,
  summary: '[private preparation trace omitted from export]',
};

const judgeReasoning = [{ id: '1:r1', kind: 'summary', text: 'Public synthetic summary.' }];

const stageAttempt = { index: 1, status: 'accepted', wallTimeMs: 60000, error: null };

const highSolveTraceAnswerA =
  'Phase one: survey readers. Phase two: draft the layout. Phase three: pick materials.';
const highSolveTraceAnswerB = 'Sketch a modular plan and iterate with visitors.';
const highSolveTraceAnswerNeutral = 'General plan: room, lighting, schedule.';

function stage(id: string, st: string, person: number | null, label: string, trace: object) {
  return { id, stage: st, person, label, trace, attempts: [{ ...stageAttempt }] };
}

function assessments(criterionId: string, status: string) {
  const one = {
    criterionId,
    status,
    reason: 'Observable coverage of the task',
    quote: 'plan',
  };
  return [{ ...one }, { ...one }];
}

function criterionScore(criterionId: string, status: string, lower: number, upper: number) {
  return {
    criterionId,
    assessments: assessments(criterionId, status),
    disputed: false,
    interval: { lower, upper },
  };
}

function evaluations(personId: string, specs: ReadonlyArray<readonly [string, string]>) {
  const criteria = specs.map(([criterionId, status]) => ({
    criterionId,
    status,
    reason: 'Observable coverage of the task',
    quote: 'plan',
  }));
  return [
    { personId, criteria: criteria.map((c) => ({ ...c })) },
    { personId, criteria: criteria.map((c) => ({ ...c })) },
  ];
}

function trialScore(
  personId: string,
  interval: { lower: number; upper: number },
  specs: ReadonlyArray<readonly [string, string, number, number]>,
) {
  return {
    version: 2,
    personId,
    interval,
    criteria: specs.map(([criterionId, status, lower, upper]) =>
      criterionScore(criterionId, status, lower, upper),
    ),
    requiredStatus: 'passed',
    judgeDisagreement: false,
  };
}

function scoreSummary(personId: string, point: number) {
  return {
    version: 2,
    personId,
    interval: { lower: point, upper: point },
    point,
    planned: 1,
    evaluated: 1,
    complete: true,
    spread: { lower: point, upper: point },
    requiredStatus: 'passed',
    judgeDisagreement: false,
    comparisonEligible: true,
  };
}

function solveTrial(
  id: string,
  personId: string,
  answer: string,
  scoreSpecs: ReadonlyArray<readonly [string, string, number, number]>,
  evalSpecs: ReadonlyArray<readonly [string, string]>,
) {
  const point = scoreSpecs.reduce((sum, [, , lower]) => sum + lower * 50, 0);
  return {
    id,
    round: 1,
    personId,
    status: 'evaluated',
    attempt: { answer, trace: { ...traceHigh } },
    score: trialScore(personId, { lower: point, upper: point }, scoreSpecs),
    evaluations: evaluations(personId, evalSpecs),
    error: null,
  };
}

function decodeJsonClone<T>(value: T): T {
  return JSON.parse(JSON.stringify(value)) as T;
}

function expectInvalid(value: unknown, detail: string) {
  expect(() => decodeLegacySeriesResult(value)).toThrowError(
    new RegExp(`Invalid legacy series result:.*${detail}`),
  );
}

/** Recursively freeze a decoded-JSON-shaped value to prove the reader never mutates input. */
function deepFreeze<T>(value: T): T {
  if (value && typeof value === 'object') {
    for (const key of Object.keys(value)) {
      deepFreeze((value as Record<string, unknown>)[key]);
    }
    Object.freeze(value);
  }
  return value;
}

const expectedV2 = {
  schemaVersion: 2,
  scoringVersion: 2,
  status: 'complete',
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
  protocol: {
    version: 2,
    task: 'Design a public reading room',
    rubric: {
      version: 2,
      criteria: [
        {
          id: 'coverage',
          label: 'Task coverage',
          weight: 1,
          required: false,
          anchors: { unmet: 'No solution', partial: 'Incomplete plan', met: 'Complete plan' },
        },
        {
          id: 'feasibility',
          label: 'Feasibility',
          weight: 1,
          required: true,
          anchors: { unmet: 'Not feasible', partial: 'Partially feasible', met: 'Fully feasible' },
        },
      ],
    },
    repetitions: 1,
    participantOrder: [['a', 'b', 'neutral']],
    modelSettings: Object.fromEntries<unknown>([
      ['stalker', { provider: 'codex', requestedModel: '', requestedEffort: 'medium' }],
      ['person-fabric', { provider: 'codex', requestedModel: '', requestedEffort: 'medium' }],
      ['person', { provider: 'codex', requestedModel: '', requestedEffort: 'high' }],
      ['brain-analyzator', { provider: 'codex', requestedModel: '', requestedEffort: 'high' }],
    ]),
    formatAttempts: 2,
    stageTimeoutMs: 600000,
    promptVersion: 'bsc-v2.1',
    createdAt: '2026-11-08T10:00:00.000Z',
  },
  people: [] as unknown[],
  control: {} as unknown,
  trials: [] as unknown[],
  comparison: {
    summary:
      'Предварительный эксперимент: один прогон. Баллы описывают выполнение критериев, устойчивость не установлена.',
    difference: { lower: 25, upper: 25 },
    comparisonEligible: false,
  },
  traces: [] as unknown[],
  finishedAt: '2026-11-08T10:24:36.000Z',
};

const v2PersonABaseProfile = {
  id: 'a',
  label: 'Ada Sample',
  palette: 'turquoise',
  archetype: 'Careful planner (synthetic)',
  skills: ['Plan in milestones'],
  rules: [
    {
      id: 'r1',
      instruction: 'Plan before acting',
      evidenceIds: ['e1'],
      status: 'hypothesis',
      limitations: ['Synthetic rule'],
    },
  ],
  taskSkills: [
    {
      id: 's1',
      instruction: 'Use a phased plan',
      evidenceIds: ['e1'],
      status: 'hypothesis',
      limitations: ['Synthetic rule'],
    },
  ],
  sourceStatus: 'synthetic',
};

const v2PersonBBaseProfile = {
  id: 'b',
  label: 'Boris Fixture',
  palette: 'violet',
  archetype: 'Rapid improviser (synthetic)',
  skills: ['Prototype early'],
  rules: [
    {
      id: 'r1',
      instruction: 'Prototype first',
      evidenceIds: ['e1'],
      status: 'hypothesis',
      limitations: ['Synthetic rule'],
    },
  ],
  taskSkills: [
    {
      id: 's1',
      instruction: 'Iterate with visitors',
      evidenceIds: ['e1'],
      status: 'hypothesis',
      limitations: ['Synthetic rule'],
    },
  ],
  sourceStatus: 'synthetic',
};

const v2NeutralProfile = {
  id: 'neutral',
  label: 'Нейтральный контроль',
  palette: 'turquoise',
  archetype: 'Решайте общую задачу без приписанных персональных черт, привычек или способностей.',
  skills: [],
  rules: [],
  taskSkills: [],
  sourceStatus: 'neutral',
};

const neutralStrategy =
  'Plan in phases: outline the requirements, draft the layout, then fix the schedule and lighting.';
const adaptedNeutral = {
  ...v2NeutralProfile,
  skills: [neutralStrategy],
  taskSkills: [
    {
      id: 's1',
      instruction: neutralStrategy,
      evidenceIds: [],
      status: 'hypothesis',
      limitations: [],
    },
  ],
};
const neutralSkillArtifact =
  "---\nname: persona-1\ndescription: Behavioral strategy for this persona's task\n---\n\n" +
  neutralStrategy +
  '\n\n- s1: ' +
  neutralStrategy +
  '\n  Status: hypothesis; evidence: none (task strategy)\n  Limitations: No additional limitations reported; not independently verified.\n';

const v2ReportA = {
  evidence: [
    {
      id: 'e1',
      sourceUrl: 'https://sample.example/ada-notes',
      kind: 'fact',
      summary: 'Plans each milestone before acting',
      status: 'supported',
      verification: 'model-reported',
      excerpt: 'Plans each milestone before acting.',
      retrievedAt: null,
      limitations: [],
    },
    {
      id: 'e2',
      sourceUrl: '',
      kind: 'hypothesis',
      summary: 'Prefers quiet work environments',
      status: 'hypothesis',
      verification: 'unverified',
      excerpt: null,
      retrievedAt: null,
      limitations: ['User description only'],
    },
  ],
  character: 'Synthetic character',
  thinkingStyle: 'Synthetic thinking style',
  activity: 'Synthetic activity',
  behaviorSummary: 'Synthetic behavior summary',
};

const v2ReportB = {
  evidence: [
    {
      id: 'e1',
      sourceUrl: '',
      kind: 'hypothesis',
      summary: 'Tests ideas quickly',
      status: 'hypothesis',
      verification: 'unverified',
      excerpt: null,
      retrievedAt: null,
      limitations: ['User description only'],
    },
  ],
  character: 'Synthetic character',
  thinkingStyle: 'Synthetic thinking style',
  activity: 'Synthetic activity',
  behaviorSummary: 'Synthetic behavior summary',
};

const v2ArtifactsA = Object.fromEntries<unknown>([
  [
    'AGENTS.md',
    '# Simulated persona\n\nSource status: synthetic\n\nCareful planner (synthetic)\n\n## Evidence and limitations\n\n- r1: Plan before acting\n  Status: hypothesis; evidence: e1\n  Limitations: Synthetic rule\n\nUse the attached skills to approach the task. This is a behavioral simulation, not the real person. Source support is model-reported, not independent verification.\n',
  ],
  [
    '.agents/skills/persona-1/SKILL.md',
    "---\nname: persona-1\ndescription: Behavioral strategy for this persona's task\n---\n\nPlan in milestones\n\n- s1: Use a phased plan\n  Status: hypothesis; evidence: e1\n  Limitations: Synthetic rule\n",
  ],
]);

const v2ArtifactsB = Object.fromEntries<unknown>([
  [
    'AGENTS.md',
    '# Simulated persona\n\nSource status: synthetic\n\nRapid improviser (synthetic)\n\n## Evidence and limitations\n\n- r1: Prototype first\n  Status: hypothesis; evidence: e1\n  Limitations: Synthetic rule\n\nUse the attached skills to approach the task. This is a behavioral simulation, not the real person. Source support is model-reported, not independent verification.\n',
  ],
  [
    '.agents/skills/persona-1/SKILL.md',
    "---\nname: persona-1\ndescription: Behavioral strategy for this persona's task\n---\n\nPrototype early\n\n- s1: Iterate with visitors\n  Status: hypothesis; evidence: e1\n  Limitations: Synthetic rule\n",
  ],
]);

const v2ArtifactsNeutral = Object.fromEntries<unknown>([
  ['.agents/skills/persona-1/SKILL.md', neutralSkillArtifact],
  [
    'AGENTS.md',
    '# Simulated persona\n\nSource status: neutral\n\nРешайте общую задачу без приписанных персональных черт, привычек или способностей.\n\n## Evidence and limitations\n\nInsufficient personal evidence; no characteristic traits asserted.\n\nUse the attached skills to approach the task. This is a behavioral simulation, not the real person. Source support is model-reported, not independent verification.\n',
  ],
]);

const v2TrialA = solveTrial(
  '1-a',
  'a',
  highSolveTraceAnswerA,
  [
    ['coverage', 'met', 1, 1],
    ['feasibility', 'met', 1, 1],
  ],
  [
    ['coverage', 'met'],
    ['feasibility', 'met'],
  ],
);

const v2TrialB = solveTrial(
  '1-b',
  'b',
  highSolveTraceAnswerB,
  [
    ['coverage', 'partial', 0.5, 0.5],
    ['feasibility', 'met', 1, 1],
  ],
  [
    ['coverage', 'partial'],
    ['feasibility', 'met'],
  ],
);

const v2TrialNeutral = solveTrial(
  '1-neutral',
  'neutral',
  highSolveTraceAnswerNeutral,
  [
    ['coverage', 'partial', 0.5, 0.5],
    ['feasibility', 'met', 1, 1],
  ],
  [
    ['coverage', 'partial'],
    ['feasibility', 'met'],
  ],
);

function v2Person(
  report: object,
  baseProfile: object,
  answer: string,
  point: number,
  trial: object,
  artifacts: object,
  baseline: { lower: number; upper: number },
) {
  return {
    report,
    baseProfile: { ...baseProfile, skills: [], taskSkills: [] },
    profile: decodeJsonClone(baseProfile),
    attempt: { answer, trace: { ...traceHigh } },
    score: scoreSummary((baseProfile as { id: string }).id, point),
    trials: [trial],
    preparationWallTimeMs: 180000,
    solutionWallTimeMs: 60000,
    baselineDifference: baseline,
    artifacts,
  };
}

expectedV2.people = [
  v2Person(v2ReportA, v2PersonABaseProfile, highSolveTraceAnswerA, 100, v2TrialA, v2ArtifactsA, {
    lower: 25,
    upper: 25,
  }),
  v2Person(v2ReportB, v2PersonBBaseProfile, highSolveTraceAnswerB, 75, v2TrialB, v2ArtifactsB, {
    lower: 0,
    upper: 0,
  }),
];

expectedV2.control = {
  report: null,
  baseProfile: v2NeutralProfile,
  profile: decodeJsonClone(adaptedNeutral),
  attempt: { answer: highSolveTraceAnswerNeutral, trace: { ...traceHigh } },
  score: scoreSummary('neutral', 75),
  trials: [v2TrialNeutral],
  preparationWallTimeMs: 60000,
  solutionWallTimeMs: 60000,
  baselineDifference: null,
  artifacts: v2ArtifactsNeutral,
};

expectedV2.trials = [v2TrialA, v2TrialB, v2TrialNeutral];

expectedV2.traces = [
  stage('rubric', 'brain-analyzator', null, 'Критерии задачи', { ...traceHigh }),
  stage('sources-0', 'stalker', 0, 'Наблюдения и источники', { ...traceMedium }),
  stage('profile-0', 'person-fabric', 0, 'Устойчивый профиль', { ...traceMedium }),
  stage('sources-1', 'stalker', 1, 'Наблюдения и источники', { ...traceMedium }),
  stage('profile-1', 'person-fabric', 1, 'Устойчивый профиль', { ...traceMedium }),
  stage('adapt-0', 'person-fabric', 0, 'Стратегия для задачи', { ...traceMedium }),
  stage('adapt-1', 'person-fabric', 1, 'Стратегия для задачи', { ...traceMedium }),
  stage('adapt-2', 'person-fabric', 2, 'Стратегия для задачи', { ...traceMedium }),
  stage('solve-1-a', 'person', 0, 'Решение · прогон 1', { ...traceHigh }),
  stage('solve-1-b', 'person', 1, 'Решение · прогон 1', { ...traceHigh }),
  stage('solve-1-neutral', 'person', 2, 'Решение · прогон 1', { ...traceHigh }),
  stage('judge-1-0', 'brain-analyzator', null, 'Слепая оценка · прогон 1 · порядок 1', {
    ...traceHigh,
    reasoning: judgeReasoning,
  }),
  stage('judge-1-1', 'brain-analyzator', null, 'Слепая оценка · прогон 1 · порядок 2', {
    ...traceHigh,
    reasoning: [],
  }),
];

const v3StyleProfile = {
  version: 1,
  reasoningPatterns: [
    {
      id: 'rp1',
      instruction: 'Сначала структура, потом детали.',
      exemplarIds: ['se-a1'],
      status: 'observed',
      limitations: ['Synthetic style rule'],
    },
  ],
  voiceRules: [
    {
      id: 'vr1',
      instruction: 'Спокойный тон без преувеличений.',
      exemplarIds: ['se-a1'],
      status: 'observed',
      limitations: [],
    },
  ],
  formatRules: [
    {
      id: 'fr1',
      instruction: 'Короткие абзацы и списки.',
      exemplarIds: [],
      status: 'hypothesis',
      limitations: [],
    },
  ],
  contextModes: [{ mode: 'comment', status: 'observed', limitations: [] }],
  styleExemplarIds: ['se-a1', 'se-a2'],
  limitations: ['Synthetic style profile'],
};

const v3StyleExemplars = [
  {
    id: 'se-a1',
    documentId: 'doc-a-public',
    mode: 'comment',
    text: 'Short public note from the synthetic thread.',
    context: 'Public thread (synthetic)',
    redactions: [],
    exportAllowed: true,
  },
];

const v3Task = 'Спроектируйте читальный зал «Лира».\r\nСезонная подсветка — обязательное условие.';

const v3AnswerA =
  'Опираюсь на набросок: «[private source text omitted from export]» Учту сезонную подсветку и расписание.';
const v3AnswerB = 'Соберу модульную схему зала и проверю её на синтетических данных.';
const v3AnswerNeutral = 'Общий план: зал, освещение, расписание работ.';

const v3ReportA = {
  evidence: [
    {
      id: 'e1',
      sourceUrl: 'https://sample.example/ada-notes',
      kind: 'fact',
      summary: 'Plans each milestone before acting',
      status: 'supported',
      verification: 'model-reported',
      excerpt: 'Plans each milestone before acting.',
      retrievedAt: null,
      limitations: [],
    },
    {
      id: 'e2',
      sourceUrl: 'https://sources.example/ada/2024-11',
      kind: 'fact',
      summary: 'Seasonal lighting requirements (connected source)',
      status: 'supported',
      verification: 'source-retrieved',
      documentIds: ['doc-a-private'],
      excerpt: null,
      retrievedAt: '2026-11-08T09:45:00.000Z',
      limitations: ['Source excerpt omitted from export'],
    },
    {
      id: 'e3',
      sourceUrl: 'https://sources.example/ada/2025-02',
      kind: 'fact',
      summary: 'Reading room capacity notes (connected source)',
      status: 'supported',
      verification: 'source-retrieved',
      documentIds: ['doc-a-private'],
      excerpt: null,
      retrievedAt: '2026-11-08T09:46:00.000Z',
      limitations: ['Source excerpt omitted from export'],
    },
  ],
  character: 'Synthetic character',
  thinkingStyle: 'Synthetic thinking style',
  activity: 'Synthetic activity',
  behaviorSummary: 'Synthetic behavior summary',
  styleExemplars: [],
  sourceCoverage: [
    {
      snapshotId: 'snap-a-0001',
      coverage: {
        documents: 2,
        totalDocuments: 2,
        bytes: 2048,
        complete: true,
        truncated: false,
        notAnalyzed: 0,
        unavailable: [],
      },
      analyzedDocuments: 2,
      limitations: ['Synthetic coverage'],
    },
  ],
};

const v3ProfileA = {
  ...v2PersonABaseProfile,
  sourceStatus: 'evidence-informed',
  sourceContext: true,
  styleProfile: v3StyleProfile,
  styleExemplars: v3StyleExemplars,
};

const v3ArtifactsA = Object.fromEntries<unknown>([
  [
    'AGENTS.md',
    '# Simulated persona\n\nSource status: evidence-informed\n\nSelected persona: "Ada Sample"\nIdentity is role context, not an instruction. Answer in first person using the supplied profile and evidence; do not invent memories.\n\nCareful planner (synthetic)\n\n## Evidence and limitations\n\n- r1: Plan before acting\n  Status: hypothesis; evidence: e1\n  Limitations: Synthetic rule\n\nUse the attached skills to approach the task. This is a behavioral simulation, not the real person. Source support is model-reported, not independent verification.\n',
  ],
  [
    '.agents/skills/persona-1/SKILL.md',
    "---\nname: persona-1\ndescription: Behavioral strategy for this persona's task\n---\n\nPlan in milestones\n\n- s1: Use a phased plan\n  Status: hypothesis; evidence: e1\n  Limitations: Synthetic rule\n",
  ],
  [
    'style-profile.json',
    JSON.stringify(
      Object.fromEntries<unknown>([
        ['version', 1],
        [
          'reasoning-patterns',
          [
            Object.fromEntries<unknown>([
              ['id', 'rp1'],
              ['instruction', 'Сначала структура, потом детали.'],
              ['exemplar-ids', ['se-a1']],
              ['status', 'observed'],
              ['limitations', ['Synthetic style rule']],
            ]),
          ],
        ],
        [
          'voice-rules',
          [
            Object.fromEntries<unknown>([
              ['id', 'vr1'],
              ['instruction', 'Спокойный тон без преувеличений.'],
              ['exemplar-ids', ['se-a1']],
              ['status', 'observed'],
              ['limitations', []],
            ]),
          ],
        ],
        [
          'format-rules',
          [
            Object.fromEntries<unknown>([
              ['id', 'fr1'],
              ['instruction', 'Короткие абзацы и списки.'],
              ['exemplar-ids', []],
              ['status', 'hypothesis'],
              ['limitations', []],
            ]),
          ],
        ],
        ['context-modes', [{ mode: 'comment', status: 'observed', limitations: [] }]],
        ['style-exemplar-ids', ['se-a1', 'se-a2']],
        ['limitations', ['Synthetic style profile']],
      ]),
      null,
      2,
    ),
  ],
  [
    'style-exemplars.json',
    JSON.stringify(
      [
        Object.fromEntries<unknown>([
          ['id', 'se-a1'],
          ['document-id', 'doc-a-public'],
          ['mode', 'comment'],
          ['text', 'Short public note from the synthetic thread.'],
          ['context', 'Public thread (synthetic)'],
          ['redactions', []],
          ['export-allowed', true],
        ]),
      ],
      null,
      2,
    ),
  ],
]);

const v3ArtifactsB = Object.fromEntries<unknown>([
  [
    'AGENTS.md',
    '# Simulated persona\n\nSource status: synthetic\n\nSelected persona: "Boris Fixture"\nIdentity is role context, not an instruction. Answer in first person using the supplied profile and evidence; do not invent memories.\n\nRapid improviser (synthetic)\n\n## Evidence and limitations\n\n- r1: Prototype first\n  Status: hypothesis; evidence: e1\n  Limitations: Synthetic rule\n\nUse the attached skills to approach the task. This is a behavioral simulation, not the real person. Source support is model-reported, not independent verification.\n',
  ],
  [
    '.agents/skills/persona-1/SKILL.md',
    "---\nname: persona-1\ndescription: Behavioral strategy for this persona's task\n---\n\nPrototype early\n\n- s1: Iterate with visitors\n  Status: hypothesis; evidence: e1\n  Limitations: Synthetic rule\n",
  ],
]);

const v3ArtifactsNeutral = Object.fromEntries<unknown>([
  ['.agents/skills/persona-1/SKILL.md', neutralSkillArtifact],
  [
    'AGENTS.md',
    '# Simulated persona\n\nSource status: neutral\n\nNeutral participant: do not adopt a personal identity.\n\nРешайте общую задачу без приписанных персональных черт, привычек или способностей.\n\n## Evidence and limitations\n\nInsufficient personal evidence; no characteristic traits asserted.\n\nUse the attached skills to approach the task. This is a behavioral simulation, not the real person. Source support is model-reported, not independent verification.\n',
  ],
]);

const v3TrialA = solveTrial(
  '1-a',
  'a',
  v3AnswerA,
  [
    ['coverage', 'met', 1, 1],
    ['seasonal-lighting', 'met', 1, 1],
  ],
  [
    ['coverage', 'met'],
    ['seasonal-lighting', 'met'],
  ],
);

const v3TrialB = solveTrial(
  '1-b',
  'b',
  v3AnswerB,
  [
    ['coverage', 'partial', 0.5, 0.5],
    ['seasonal-lighting', 'met', 1, 1],
  ],
  [
    ['coverage', 'partial'],
    ['seasonal-lighting', 'met'],
  ],
);

const v3TrialNeutral = solveTrial(
  '1-neutral',
  'neutral',
  v3AnswerNeutral,
  [
    ['coverage', 'partial', 0.5, 0.5],
    ['seasonal-lighting', 'met', 1, 1],
  ],
  [
    ['coverage', 'partial'],
    ['seasonal-lighting', 'met'],
  ],
);

const v3Trials = [v3TrialA, v3TrialB, v3TrialNeutral];
for (const trial of v3Trials)
  Object.assign(trial.attempt, {
    reply: { version: 1, messages: [trial.attempt.answer], styleConflicts: [] },
  });

function v3Person(
  report: object | null,
  baseProfile: object,
  answer: string,
  point: number,
  trials: ReadonlyArray<object>,
  artifacts: object,
  baseline: { lower: number; upper: number } | null,
  prepMs: number,
) {
  return {
    report,
    baseProfile: { ...baseProfile, skills: [], taskSkills: [] },
    profile: decodeJsonClone(baseProfile),
    attempt: answer
      ? {
          answer,
          trace: { ...traceHigh },
          reply: { version: 1, messages: [answer], styleConflicts: [] },
        }
      : null,
    score: scoreSummary((baseProfile as { id: string }).id, point),
    trials: trials.map((trial) => trial),
    preparationWallTimeMs: prepMs,
    solutionWallTimeMs: 60000,
    baselineDifference: baseline,
    artifacts,
  };
}

const expectedV3 = {
  schemaVersion: 3,
  scoringVersion: 2,
  status: 'complete',
  input: {
    task: v3Task,
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
  protocol: {
    version: 2,
    task: v3Task,
    rubric: {
      version: 2,
      criteria: [
        {
          id: 'coverage',
          label: 'Task coverage',
          weight: 1,
          required: false,
          anchors: { unmet: 'No solution', partial: 'Incomplete plan', met: 'Complete plan' },
        },
        {
          id: 'seasonal-lighting',
          label: 'Seasonal lighting',
          weight: 1,
          required: true,
          anchors: {
            unmet: 'No lighting plan',
            partial: 'Partial lighting plan',
            met: 'Full lighting plan',
          },
        },
      ],
    },
    repetitions: 1,
    participantOrder: [['a', 'b', 'neutral']],
    modelSettings: Object.fromEntries<unknown>([
      ['stalker', { provider: 'codex', requestedModel: '', requestedEffort: 'medium' }],
      ['person-fabric', { provider: 'codex', requestedModel: '', requestedEffort: 'medium' }],
      ['person', { provider: 'codex', requestedModel: '', requestedEffort: 'high' }],
      ['brain-analyzator', { provider: 'codex', requestedModel: '', requestedEffort: 'high' }],
    ]),
    formatAttempts: 2,
    stageTimeoutMs: 600000,
    promptVersion: 'bsc-v2.3-source-identity',
    createdAt: '2026-11-08T10:00:00.000Z',
    sourceSnapshots: [['snap-a-0001'], []],
  },
  people: [
    v3Person(
      v3ReportA,
      v3ProfileA,
      v3AnswerA,
      100,
      [v3TrialA],
      v3ArtifactsA,
      {
        lower: 25,
        upper: 25,
      },
      180000,
    ),
    v3Person(
      v2ReportB,
      v2PersonBBaseProfile,
      v3AnswerB,
      75,
      [v3TrialB],
      v3ArtifactsB,
      {
        lower: 0,
        upper: 0,
      },
      180000,
    ),
  ],
  control: v3Person(
    null,
    adaptedNeutral,
    v3AnswerNeutral,
    75,
    [v3TrialNeutral],
    v3ArtifactsNeutral,
    null,
    60000,
  ),
  trials: v3Trials,
  comparison: {
    summary:
      'Предварительный эксперимент: один прогон. Баллы описывают выполнение критериев, устойчивость не установлена.',
    difference: { lower: 25, upper: 25 },
    comparisonEligible: false,
  },
  traces: [
    stage('rubric', 'brain-analyzator', null, 'Критерии задачи', { ...traceHigh }),
    stage('sources-0', 'stalker', 0, 'Наблюдения и источники', { ...traceRedactedPrep }),
    stage('profile-0', 'person-fabric', 0, 'Устойчивый профиль', { ...traceRedactedPrep }),
    stage('sources-1', 'stalker', 1, 'Наблюдения и источники', { ...traceRedactedPrep }),
    stage('profile-1', 'person-fabric', 1, 'Устойчивый профиль', { ...traceRedactedPrep }),
    stage('adapt-0', 'person-fabric', 0, 'Стратегия для задачи', { ...traceRedactedPrep }),
    stage('adapt-1', 'person-fabric', 1, 'Стратегия для задачи', { ...traceRedactedPrep }),
    stage('adapt-2', 'person-fabric', 2, 'Стратегия для задачи', { ...traceRedactedPrep }),
    stage('solve-1-a', 'person', 0, 'Решение · прогон 1', { ...traceHigh }),
    stage('solve-1-b', 'person', 1, 'Решение · прогон 1', { ...traceHigh }),
    stage('solve-1-neutral', 'person', 2, 'Решение · прогон 1', { ...traceHigh }),
    stage('judge-1-0', 'brain-analyzator', null, 'Слепая оценка · прогон 1 · порядок 1', {
      ...traceHigh,
      reasoning: judgeReasoning,
    }),
    stage('judge-1-1', 'brain-analyzator', null, 'Слепая оценка · прогон 1 · порядок 2', {
      ...traceHigh,
      reasoning: [],
    }),
  ],
  finishedAt: '2026-11-08T10:24:36.000Z',
  exportMetadata: {
    version: 1,
    redacted: true,
    omittedStyleExemplars: 2,
    omittedSourceExcerpts: 2,
    preparationReasoningOmitted: true,
    answerTextMayBeRedacted: true,
    scoresReferenceOriginalAnswers: true,
    limitations: [
      'Connected-source excerpts and private sample objects with their context are omitted. Matching text of at least 20 characters is replaced elsewhere; short quotations inside generated answers and paraphrases are not detected.',
      'Scores describe the original in-memory answers; redacted exported answers are not a lossless replay.',
    ],
  },
};

type JsonMap = Record<string, unknown>;

function mutableClone(value: unknown): JsonMap {
  return JSON.parse(JSON.stringify(value)) as JsonMap;
}

function map(value: unknown): JsonMap {
  return value as JsonMap;
}

function list(value: unknown): JsonMap[] {
  return value as JsonMap[];
}

function participantsOf(raw: JsonMap): JsonMap[] {
  return [...list(raw['people']), map(raw['control'])];
}

function solveAttemptsOf(raw: JsonMap): JsonMap[] {
  const attempts: JsonMap[] = list(raw['trials']).flatMap((trial) =>
    trial['attempt'] ? [map(trial['attempt'])] : [],
  );
  for (const person of participantsOf(raw)) {
    if (person['attempt']) attempts.push(map(person['attempt']));
    for (const trial of list(person['trials'])) {
      if (trial['attempt']) attempts.push(map(trial['attempt']));
    }
  }
  return attempts;
}

function withReply(attempt: JsonMap): void {
  const answer = attempt['answer'] as string;
  attempt['reply'] = Object.fromEntries<unknown>([
    ['version', 1],
    ['messages', [answer]],
    ['style-conflicts', []],
  ]);
}

const notCompleteSummary =
  'Серия не завершена. Баллы относятся только к оценённым ответам; итоговое сравнение недоступно.';

function emptiedSummary(score: JsonMap): void {
  score['interval'] = null;
  score['point'] = null;
  score['evaluated'] = 0;
  score['complete'] = false;
  score['spread'] = null;
  score['required-status'] = 'unknown';
  score['judge-disagreement'] = false;
  score['comparison-eligible'] = false;
}

describe('exact fixture projections', () => {
  it('decodes the v2 raw-save fixture to the exact camelCase projection', () => {
    expect(decodeLegacySeriesResult(v2Fixture)).toEqual(expectedV2);
  });

  it('decodes the v3 redacted export fixture to the exact camelCase projection', () => {
    expect(decodeLegacySeriesResult(v3Fixture)).toEqual(expectedV3);
  });

  it('preserves recorded BSC values and export redactions without restoring source text', () => {
    const decoded = decodeLegacySeriesResult(v3Fixture);
    expect(decoded.people[0].score.interval).toEqual({ lower: 100, upper: 100 });
    expect(decoded.people[1].score.interval).toEqual({ lower: 75, upper: 75 });
    expect(decoded.control.score.interval).toEqual({ lower: 75, upper: 75 });
    expect(decoded.comparison.difference).toEqual({ lower: 25, upper: 25 });
    // Redaction markers stay redacted; no source material is re-invented.
    expect(decoded.people[0].attempt!.answer).toBe(v3AnswerA);
    expect(decoded.people[0].attempt!.answer).toContain(
      '[private source text omitted from export]',
    );
    const e2 = decoded.people[0].report!.evidence[1]!;
    expect(e2.verification).toBe('source-retrieved');
    expect(e2.excerpt).toBeNull();
    expect(e2.limitations).toEqual(['Source excerpt omitted from export']);
    // Dangling exemplar ids after private exemplar removal are preserved as recorded.
    expect(decoded.people[0].profile.styleProfile!.styleExemplarIds).toEqual(['se-a1', 'se-a2']);
    expect(decoded.people[0].profile.styleExemplars!.map((sample) => sample.id)).toEqual(['se-a1']);
    expect(decoded.people[0].report!.styleExemplars).toEqual([]);
    // Writer-owned export metadata is opaque but decoded exactly.
    expect(decoded.exportMetadata).toEqual(expectedV3.exportMetadata);
    // Preparation traces keep the writer placeholder summary.
    const sourcesTrace = decoded.traces.find((entry) => entry.id === 'sources-0')!;
    expect(sourcesTrace.trace.summary).toBe('[private preparation trace omitted from export]');
    expect(sourcesTrace.trace.reasoning).toEqual([]);
  });

  it('keeps missing optional fields absent on both schemas', () => {
    const v2 = decodeLegacySeriesResult(v2Fixture);
    expect(v2).not.toHaveProperty('exportMetadata');
    expect(v2.protocol).not.toHaveProperty('sourceSnapshots');
    expect(v2.people[0].report!.evidence[0]).not.toHaveProperty('documentIds');
    expect(v2.people[0].profile).not.toHaveProperty('styleProfile');
    expect(v2.people[0].profile).not.toHaveProperty('styleExemplars');
    expect(v2.people[0].profile).not.toHaveProperty('sourceContext');
    expect(v2.people[0].report).not.toHaveProperty('styleExemplars');
    expect(v2.people[0]).not.toHaveProperty('sourceCoverage');
    expect(v2.people[0].attempt).not.toHaveProperty('reply');
    const v3 = decodeLegacySeriesResult(v3Fixture);
    expect(v3.people[1].profile).not.toHaveProperty('styleProfile');
    expect(v3.people[1].report).not.toHaveProperty('styleExemplars');
    expect(v3.people[1].report).not.toHaveProperty('sourceCoverage');
    expect(v3.control.report).toBeNull();
    expect(v3.people[0].attempt!.reply?.version).toBe(1);
  });

  it('accepts the v3 raw-save variant: earlier prompt version, no export metadata, with reply', () => {
    const raw = mutableClone(v3Fixture);
    delete raw['export-metadata'];
    map(raw['protocol'])['prompt-version'] = 'bsc-v2.2-source-style';
    for (const attempt of solveAttemptsOf(raw)) withReply(attempt);
    const decoded = decodeLegacySeriesResult(raw);
    expect(decoded.schemaVersion).toBe(3);
    expect(decoded.protocol.promptVersion).toBe('bsc-v2.2-source-style');
    expect(decoded.exportMetadata).toBeUndefined();
    expect(decoded.people[0].attempt!.reply).toEqual({
      version: 1,
      messages: [v3AnswerA],
      styleConflicts: [],
    });
    expect(decoded.control.attempt!.reply).toEqual({
      version: 1,
      messages: [v3AnswerNeutral],
      styleConflicts: [],
    });
  });

  it('accepts the literal CRLF and Cyrillic task text byte-for-byte', () => {
    const decoded = decodeLegacySeriesResult(v3Fixture);
    expect(decoded.input.task).toBe(v3Task);
    expect(decoded.protocol.task).toBe(v3Task);
    expect(v3Task).toContain('\r\n');
    expect(v3Task).toContain('«Лира»');
  });
});

describe('versions, aliases and unknown fields', () => {
  it('rejects unsupported schema and scoring versions', () => {
    const v4 = mutableClone(v2Fixture);
    v4['schema-version'] = 4;
    expectInvalid(v4, "'schema-version' must be 2 or 3");
    const v1 = mutableClone(v2Fixture);
    v1['schema-version'] = 1;
    expectInvalid(v1, "'schema-version' must be 2 or 3");
    const unversioned = mutableClone(v2Fixture);
    delete unversioned['schema-version'];
    expectInvalid(unversioned, 'unexpected field set');
    const scoring1 = mutableClone(v2Fixture);
    scoring1['scoring-version'] = 1;
    expectInvalid(scoring1, "'scoring-version' must be 2");
    const scoring3 = mutableClone(v3Fixture);
    scoring3['scoring-version'] = 3;
    expectInvalid(scoring3, "'scoring-version' must be 2");
    const protocol3 = mutableClone(v2Fixture);
    map(protocol3['protocol'])['version'] = 3;
    expectInvalid(protocol3, "protocol 'version' must be 2");
  });

  it('rejects prompt versions from the other era', () => {
    const v2LatePrompt = mutableClone(v2Fixture);
    map(v2LatePrompt['protocol'])['prompt-version'] = 'bsc-v2.2-source-style';
    expectInvalid(v2LatePrompt, "'prompt-version' must be one of the era values");
    const v2IdentityPrompt = mutableClone(v2Fixture);
    map(v2IdentityPrompt['protocol'])['prompt-version'] = 'bsc-v2.3-source-identity';
    expectInvalid(v2IdentityPrompt, "'prompt-version' must be one of the era values");
    const v3EarlyPrompt = mutableClone(v3Fixture);
    map(v3EarlyPrompt['protocol'])['prompt-version'] = 'bsc-v2.1';
    expectInvalid(v3EarlyPrompt, "'prompt-version' must be one of the era values");
    const v4CamelPrompt = mutableClone(v3Fixture);
    map(v4CamelPrompt['protocol'])['prompt-version'] = 'bsc-v2.4-camel-case';
    expectInvalid(v4CamelPrompt, "'prompt-version' must be one of the era values");
  });

  it('rejects era-mismatched and unknown keys on exact schemas', () => {
    const v2Export = mutableClone(v2Fixture);
    v2Export['export-metadata'] = { version: 1 };
    expectInvalid(v2Export, 'unexpected field set');
    const v2Snapshots = mutableClone(v2Fixture);
    map(v2Snapshots['protocol'])['source-snapshots'] = [[], []];
    expectInvalid(v2Snapshots, 'unexpected field set');
    const v2Retrieved = mutableClone(v2Fixture);
    const evidence = list(map(list(v2Retrieved['people'])[0]!['report'])['evidence']);
    evidence[0]!['verification'] = 'source-retrieved';
    expectInvalid(v2Retrieved, "'verification' must be one of the era values");
    const v2DocumentIds = mutableClone(v2Fixture);
    const v2Evidence = list(map(list(v2DocumentIds['people'])[0]!['report'])['evidence']);
    v2Evidence[0]!['document-ids'] = ['doc-1'];
    expectInvalid(v2DocumentIds, 'unexpected field set');
    const v2Style = mutableClone(v2Fixture);
    map(list(v2Style['people'])[0]!['profile'])['style-profile'] = { version: 1 };
    expectInvalid(v2Style, 'unexpected field set');
    const v2Reply = mutableClone(v2Fixture);
    map(list(v2Reply['people'])[0]!['attempt'])['reply'] = { version: 1, messages: ['x'] };
    expectInvalid(v2Reply, 'unexpected field set');
    const unknownTop = mutableClone(v2Fixture);
    unknownTop['execution'] = null;
    expectInvalid(unknownTop, 'unexpected field set');
    const camelAlias = mutableClone(v2Fixture);
    delete camelAlias['finished-at'];
    camelAlias['finishedAt'] = '2026-11-08T10:24:36.000Z';
    expectInvalid(camelAlias, 'unexpected field set');
    const camelTrial = mutableClone(v2Fixture);
    const trial = list(camelTrial['trials'])[0]!;
    delete trial['person-id'];
    trial['personId'] = 'a';
    expectInvalid(camelTrial, 'unexpected field set');
    const mixedAliases = mutableClone(v2Fixture);
    map(list(mixedAliases['people'])[0]!['profile'])['sourceStatus'] = 'synthetic';
    expectInvalid(mixedAliases, 'unexpected field set');
    const v3MissingSnapshots = mutableClone(v3Fixture);
    delete map(v3MissingSnapshots['protocol'])['source-snapshots'];
    expect(decodeLegacySeriesResult(v3MissingSnapshots).protocol).not.toHaveProperty(
      'sourceSnapshots',
    );
  });

  it('rejects non-canonical and impossible timestamps', () => {
    const feb30 = mutableClone(v2Fixture);
    feb30['finished-at'] = '2026-02-30T10:00:00.000Z';
    expectInvalid(feb30, 'canonical UTC');
    const offset = mutableClone(v2Fixture);
    offset['finished-at'] = '2026-11-08T10:24:36+01:00';
    expectInvalid(offset, 'canonical UTC');
    const badCreated = mutableClone(v2Fixture);
    map(badCreated['protocol'])['created-at'] = '2026-13-08T10:00:00.000Z';
    expectInvalid(badCreated, 'canonical UTC');
    const badRetrieved = mutableClone(v3Fixture);
    const evidence = list(map(list(badRetrieved['people'])[0]!['report'])['evidence']);
    evidence[1]!['retrieved-at'] = 'not-a-date';
    expectInvalid(badRetrieved, 'canonical UTC');
    const numericDate = mutableClone(v2Fixture);
    numericDate['finished-at'] = 1728000000;
    expectInvalid(numericDate, 'canonical UTC');
  });
});

describe('numeric bounds and intervals', () => {
  it('rejects non-finite and out-of-range numbers', () => {
    const infinite = mutableClone(v2Fixture);
    map(map(list(infinite['people'])[0]!['attempt'])['trace'])['reasoning-ms'] = Infinity;
    expectInvalid(infinite, 'finite number');
    const reversed = mutableClone(v2Fixture);
    map(list(reversed['people'])[0]!['score'])['interval'] = { lower: 80, upper: 75 };
    expectInvalid(reversed, 'ordered');
    const above100 = mutableClone(v2Fixture);
    map(list(above100['people'])[0]!['score'])['interval'] = { lower: 0, upper: 101 };
    expectInvalid(above100, 'within 0-100');
    const negativeCriterion = mutableClone(v2Fixture);
    list(map(list(negativeCriterion['trials'])[0]!['score'])['criteria'])[0]!['interval'] = {
      lower: -0.1,
      upper: 1,
    };
    expectInvalid(negativeCriterion, 'within 0-1');
    const zeroWeight = mutableClone(v2Fixture);
    list(map(map(zeroWeight['protocol'])['rubric'])['criteria'])[0]!['weight'] = 0;
    expectInvalid(zeroWeight, 'positive');
    const unsafePlanned = mutableClone(v2Fixture);
    map(list(unsafePlanned['people'])[0]!['score'])['planned'] = Number.MAX_SAFE_INTEGER + 1;
    expectInvalid(unsafePlanned, 'safe integer');
    const evaluatedExcess = mutableClone(v2Fixture);
    map(list(evaluatedExcess['people'])[0]!['score'])['evaluated'] = 2;
    expectInvalid(evaluatedExcess, 'evaluated');
    const repetitions5 = mutableClone(v2Fixture);
    map(repetitions5['protocol'])['repetitions'] = 5;
    expectInvalid(repetitions5, "'repetitions' must be 1 or 3");
    const stageTimeout = mutableClone(v2Fixture);
    map(stageTimeout['protocol'])['stage-timeout-ms'] = 300000;
    expectInvalid(stageTimeout, "'stage-timeout-ms' must be 600000");
    const formatAttempts = mutableClone(v2Fixture);
    map(formatAttempts['protocol'])['format-attempts'] = 3;
    expectInvalid(formatAttempts, "'format-attempts' must be 2");
  });

  it('rejects rubric rule violations', () => {
    const thirteen = mutableClone(v2Fixture);
    const criteria = list(map(map(thirteen['protocol'])['rubric'])['criteria']);
    for (let i = 0; i < 11; i++) {
      criteria.push({ ...map(criteria[0]!), id: `extra-${i}` });
    }
    expectInvalid(thirteen, '1 to 12 criteria');
    const duplicateIds = mutableClone(v2Fixture);
    const dupCriteria = list(map(map(duplicateIds['protocol'])['rubric'])['criteria']);
    dupCriteria[1]!['id'] = 'coverage';
    expectInvalid(duplicateIds, 'unique');
    const nonKebab = mutableClone(v2Fixture);
    list(map(map(nonKebab['protocol'])['rubric'])['criteria'])[0]!['id'] = 'Coverage';
    expectInvalid(nonKebab, 'kebab-case');
    const blankAnchor = mutableClone(v2Fixture);
    map(list(map(map(blankAnchor['protocol'])['rubric'])['criteria'])[0]!['anchors'])['met'] = ' ';
    expectInvalid(blankAnchor, 'blank');
    const rubricVersion = mutableClone(v2Fixture);
    map(map(rubricVersion['protocol'])['rubric'])['version'] = 3;
    expectInvalid(rubricVersion, "'version' must be 2");
  });

  it('rejects invalid input and model-setting bounds', () => {
    const blankTask = mutableClone(v2Fixture);
    map(blankTask['input'])['task'] = '   ';
    expectInvalid(blankTask, "input 'task' must not be blank");
    const longTask = mutableClone(v2Fixture);
    map(longTask['input'])['task'] = 'x'.repeat(4001);
    expectInvalid(longTask, 'at most 4000 characters');
    const threePeople = mutableClone(v2Fixture);
    list(map(threePeople['input'])['people']).push(
      Object.fromEntries<unknown>([
        ['label', 'Extra'],
        ['context', 'c'],
        ['public-urls', []],
      ]),
    );
    expectInvalid(threePeople, 'exactly two people');
    const blankLabel = mutableClone(v2Fixture);
    list(map(blankLabel['input'])['people'])[0]!['label'] = '';
    expectInvalid(blankLabel, "input.people\\[0\\] 'label'");
    const sixUrls = mutableClone(v2Fixture);
    list(map(sixUrls['input'])['people'])[0]!['public-urls'] = [
      'https://sample.example/1',
      'https://sample.example/2',
      'https://sample.example/3',
      'https://sample.example/4',
      'https://sample.example/5',
      'https://sample.example/6',
    ];
    expectInvalid(sixUrls, 'at most 5 URLs');
    const missingComponent = mutableClone(v2Fixture);
    const settings = map(map(missingComponent['protocol'])['model-settings']);
    delete settings['stalker'];
    expectInvalid(missingComponent, "'model-settings' must list exactly");
    const badEffort = mutableClone(v2Fixture);
    map(map(map(badEffort['protocol'])['model-settings'])['person'])['requested-effort'] = 'max';
    expectInvalid(badEffort, "'requested-effort' must be one of");
  });
});

describe('reference integrity', () => {
  it('rejects duplicate and unknown trial references', () => {
    const duplicate = mutableClone(v2Fixture);
    const trials = list(duplicate['trials']);
    trials[1] = JSON.parse(JSON.stringify(trials[0])) as JsonMap;
    expectInvalid(duplicate, 'trial sequence');
    const unknownParticipant = mutableClone(v2Fixture);
    list(unknownParticipant['trials'])[0]!['person-id'] = 'c';
    expectInvalid(unknownParticipant, 'participant');
    const badOrder = mutableClone(v2Fixture);
    map(badOrder['protocol'])['participant-order'] = [['a', 'x', 'neutral']];
    expectInvalid(badOrder, "'participant-order' row");
    const shortOrder = mutableClone(v2Fixture);
    map(shortOrder['protocol'])['participant-order'] = [['a', 'b']];
    expectInvalid(shortOrder, "'participant-order' row");
    const swappedTrials = mutableClone(v2Fixture);
    const swapped = list(swappedTrials['trials']);
    [swapped[0], swapped[1]] = [swapped[1]!, swapped[0]!];
    expectInvalid(swappedTrials, 'trial sequence');
  });

  it('rejects conflicting copied trial and attempt identity', () => {
    const personTrialsConflict = mutableClone(v2Fixture);
    list(map(list(personTrialsConflict['people'])[0]!['trials']))[0]!['round'] = 2;
    expectInvalid(personTrialsConflict, 'conflicting copied trial');
    const attemptConflict = mutableClone(v2Fixture);
    map(map(list(attemptConflict['people'])[0]!)['attempt'])['answer'] = 'Tampered answer';
    expectInvalid(attemptConflict, 'conflicting copied attempt');
    const topLevelTrialConflict = mutableClone(v2Fixture);
    list(topLevelTrialConflict['trials'])[0]!['status'] = 'answered';
    list(topLevelTrialConflict['trials'])[0]!['score'] = null;
    expectInvalid(topLevelTrialConflict, 'conflicting copied trial');
  });

  it('rejects duplicate trace ids and wrong solve-trace references', () => {
    const duplicateTrace = mutableClone(v2Fixture);
    const traces = list(duplicateTrace['traces']);
    traces[1] = JSON.parse(JSON.stringify(traces[0])) as JsonMap;
    expectInvalid(duplicateTrace, 'duplicate trace id');
    const missingSolve = mutableClone(v2Fixture);
    const traceList = list(missingSolve['traces']);
    const index = traceList.findIndex((entry) => entry['id'] === 'solve-1-a');
    traceList.splice(index, 1);
    expectInvalid(missingSolve, "missing 'solve-1-a' trace");
    const wrongPerson = mutableClone(v2Fixture);
    const wrong = list(wrongPerson['traces']).find((entry) => entry['id'] === 'solve-1-a')!;
    wrong['person'] = 1;
    expectInvalid(wrongPerson, "'solve-1-a' trace person");
  });

  it('rejects broken evidence and wall-time references', () => {
    const unknownEvidence = mutableClone(v2Fixture);
    list(map(list(unknownEvidence['people'])[0]!['profile'])['rules'])[0]!['evidence-ids'] = [
      'nope',
    ];
    expectInvalid(unknownEvidence, 'unknown evidence id');
    const duplicateEvidence = mutableClone(v2Fixture);
    const evidence = list(map(list(duplicateEvidence['people'])[0]!['report'])['evidence']);
    evidence[1]!['id'] = 'e1';
    expectInvalid(duplicateEvidence, 'duplicate evidence id');
    const wallTime = mutableClone(v2Fixture);
    map(list(wallTime['people'])[0]!)['preparation-wall-time-ms'] = 60000;
    expectInvalid(wallTime, "'preparation-wall-time-ms'");
    const solutionWall = mutableClone(v2Fixture);
    map(map(solutionWall['control']))['solution-wall-time-ms'] = 60001;
    expectInvalid(solutionWall, "'solution-wall-time-ms'");
  });

  it('rejects incoherent score flags and baseline references', () => {
    const eligibleOnSingle = mutableClone(v2Fixture);
    map(eligibleOnSingle['comparison'])['comparison-eligible'] = true;
    expect(decodeLegacySeriesResult(eligibleOnSingle).comparison.comparisonEligible).toBe(true);
    const baselineWithoutControl = mutableClone(v2Fixture);
    const controlScore = map(map(baselineWithoutControl['control'])['score']);
    controlScore['interval'] = null;
    controlScore['point'] = null;
    controlScore['spread'] = null;
    controlScore['evaluated'] = 0;
    controlScore['complete'] = false;
    controlScore['required-status'] = 'unknown';
    expect(decodeLegacySeriesResult(baselineWithoutControl).people[0].baselineDifference).toEqual({
      lower: 25,
      upper: 25,
    });
    const pointWithoutInterval = mutableClone(v2Fixture);
    map(list(pointWithoutInterval['people'])[0]!['score'])['interval'] = null;
    expectInvalid(pointWithoutInterval, "'point' requires");
    const completeWhenPartial = mutableClone(v2Fixture);
    map(list(completeWhenPartial['people'])[0]!['score'])['evaluated'] = 0;
    map(list(completeWhenPartial['people'])[0]!['score'])['complete'] = true;
    expectInvalid(completeWhenPartial, "'complete'");
    const trialScorePerson = mutableClone(v2Fixture);
    map(list(trialScorePerson['trials'])[0]!['score'])['person-id'] = 'b';
    expectInvalid(trialScorePerson, "'person-id' mismatch");
    const wrongCriterion = mutableClone(v2Fixture);
    const scoreCriteria = list(map(list(wrongCriterion['trials'])[0]!['score'])['criteria']);
    scoreCriteria[0]!['criterion-id'] = 'other';
    expectInvalid(wrongCriterion, 'rubric criterion');
    const missingCriterion = mutableClone(v2Fixture);
    list(map(list(missingCriterion['trials'])[0]!['score'])['criteria']).pop();
    expectInvalid(missingCriterion, 'rubric criterion');
    const assessmentStatus = mutableClone(v2Fixture);
    list(list(list(assessmentStatus['trials'])[0]!['evaluations'])[0]!['criteria'])[0]!['status'] =
      'maybe';
    expectInvalid(assessmentStatus, "'status' must be one of");
  });

  it('rejects invalid reply objects on v3 attempts', () => {
    const replyVersion = mutableClone(v3Fixture);
    withReply(map(list(replyVersion['people'])[0]!['attempt']));
    map(map(list(replyVersion['people'])[0]!['attempt'])['reply'])['version'] = 2;
    expectInvalid(replyVersion, "'reply.version' must be 1");
    const emptyMessages = mutableClone(v3Fixture);
    withReply(map(list(emptyMessages['people'])[0]!['attempt']));
    map(map(list(emptyMessages['people'])[0]!['attempt'])['reply'])['messages'] = [];
    expectInvalid(emptyMessages, "'reply.messages'");
    const brokenJoin = mutableClone(v3Fixture);
    withReply(map(list(brokenJoin['people'])[0]!['attempt']));
    map(map(list(brokenJoin['people'])[0]!['attempt'])['reply'])['messages'] = ['different text'];
    expectInvalid(brokenJoin, "'reply' messages do not reproduce the recorded answer");
  });
});

describe('detached output, opaque keys and input preservation', () => {
  it('decodes deeply frozen input without mutating it', () => {
    const frozen = deepFreeze(decodeJsonClone(v3Fixture));
    const before = JSON.stringify(frozen);
    expect(decodeLegacySeriesResult(frozen)).toEqual(expectedV3);
    expect(JSON.stringify(frozen)).toBe(before);
  });

  it('returns detached objects that do not alias input or later decodes', () => {
    const source = decodeJsonClone(v2Fixture);
    const first = decodeLegacySeriesResult(source);
    const second = decodeLegacySeriesResult(source);
    expect(first).toEqual(expectedV2);
    const people = first.people as unknown as Array<{ profile: { label: string } }>;
    people[0]!.profile.label = 'Mutated';
    (first.trials as unknown as Array<{ attempt: { answer: string } }>)[0]!.attempt.answer =
      'Mutated';
    expect(second).toEqual(expectedV2);
    expect(decodeLegacySeriesResult(source)).toEqual(expectedV2);
    expect(expectedV2.people[0]).toMatchObject({ profile: { label: 'Ada Sample' } });
  });

  it('preserves inert __proto__/constructor/prototype artifact keys', () => {
    const raw = mutableClone(v2Fixture);
    const artifacts = map(list(raw['people'])[0]!['artifacts']);
    delete artifacts['AGENTS.md'];
    const withUnsafe = JSON.parse(
      '{"AGENTS.md":"ok","__proto__":"inert-value","constructor":"ctor","prototype":"proto"}',
    ) as JsonMap;
    for (const key of Object.keys(withUnsafe)) {
      // Plain assignment would silently drop '__proto__' through the inherited
      // accessor; define each inert key as an own data property the way a
      // parsed JSON file carries it.
      Object.defineProperty(artifacts, key, {
        value: withUnsafe[key],
        enumerable: true,
        configurable: true,
        writable: true,
      });
    }
    const decoded = decodeLegacySeriesResult(raw);
    const decodedArtifacts = decoded.people[0].artifacts as Record<string, string>;
    expect(decodedArtifacts['__proto__']).toBe('inert-value');
    expect(decodedArtifacts['constructor']).toBe('ctor');
    expect(decodedArtifacts['prototype']).toBe('proto');
    expect(Object.keys(decodedArtifacts)).toEqual(
      expect.arrayContaining(['__proto__', 'constructor', 'prototype']),
    );
    expect(Object.getPrototypeOf(decodedArtifacts)).toBe(Object.prototype);
    expect(({} as Record<string, unknown>)['__proto__']).not.toBe('inert-value');
  });
});

describe('incomplete and cancelled records', () => {
  it('accepts a coherent cancelled record', () => {
    const raw = mutableClone(v2Fixture);
    raw['status'] = 'cancelled';
    map(raw['comparison'])['summary'] = notCompleteSummary;
    map(raw['comparison'])['comparison-eligible'] = false;
    map(raw['comparison'])['difference'] = null;
    const traceList = list(raw['traces']);
    const cancelledIndex = traceList.findIndex((entry) => entry['id'] === 'solve-1-a');
    traceList.splice(cancelledIndex + 1);
    const solveTrace = map(traceList.find((entry) => entry['id'] === 'solve-1-a')!);
    map(solveTrace['trace'])['summary'] = 'Запрос не завершён: Отменено';
    solveTrace['attempts'] = [
      Object.fromEntries<unknown>([
        ['index', 1],
        ['status', 'failed'],
        ['wall-time-ms', 60000],
        ['error', 'Отменено'],
      ]),
    ];
    const trials = list(raw['trials']);
    map(trials[0]!)['status'] = 'failed';
    map(trials[0]!)['attempt'] = null;
    map(trials[0]!)['score'] = null;
    map(trials[0]!)['evaluations'] = [];
    map(trials[0]!)['error'] = 'Отменено';
    for (const skipped of [trials[1]!, trials[2]!]) {
      map(skipped)['status'] = 'skipped';
      map(skipped)['attempt'] = null;
      map(skipped)['score'] = null;
      map(skipped)['evaluations'] = [];
      map(skipped)['error'] = 'Отменено';
    }
    for (const person of participantsOf(raw)) {
      person['attempt'] = null;
      emptiedSummary(map(person['score']));
      person['baseline-difference'] = null;
      person['trials'] = structuredClone(
        list(raw['trials']).filter((trial) => trial['person-id'] === map(person['profile'])['id']),
      );
    }
    map(list(raw['people'])[1]!)['solution-wall-time-ms'] = 0;
    map(raw['control'])['solution-wall-time-ms'] = 0;
    const decoded = decodeLegacySeriesResult(raw);
    expect(decoded.status).toBe('cancelled');
    expect(decoded.comparison).toEqual({
      summary: notCompleteSummary,
      difference: null,
      comparisonEligible: false,
    });
    expect(decoded.trials.map((trial) => trial.status)).toEqual(['failed', 'skipped', 'skipped']);
    expect(decoded.trials.map((trial) => trial.error)).toEqual([
      'Отменено',
      'Отменено',
      'Отменено',
    ]);
    expect(decoded.people[0].attempt).toBeNull();
    expect(decoded.people[0].score).toEqual({
      version: 2,
      personId: 'a',
      interval: null,
      point: null,
      planned: 1,
      evaluated: 0,
      complete: false,
      spread: null,
      requiredStatus: 'unknown',
      judgeDisagreement: false,
      comparisonEligible: false,
    });
    expect(decoded.control.baselineDifference).toBeNull();
    expect(decoded.traces.find((entry) => entry.id === 'solve-1-a')!.attempts[0]).toEqual({
      index: 1,
      status: 'failed',
      wallTimeMs: 60000,
      error: 'Отменено',
    });
  });

  it('accepts a coherent incomplete record with partial evaluations', () => {
    const raw = mutableClone(v2Fixture);
    raw['status'] = 'incomplete';
    map(raw['comparison'])['summary'] = notCompleteSummary;
    map(raw['comparison'])['comparison-eligible'] = false;
    map(raw['comparison'])['difference'] = null;
    const message = 'модель дважды вернула неподходящий формат JSON';
    const judgeTrace = map(list(raw['traces']).find((entry) => entry['id'] === 'judge-1-1')!);
    map(judgeTrace['trace'])['summary'] = `Запрос не завершён: ${message}`;
    judgeTrace['attempts'] = [
      Object.fromEntries<unknown>([
        ['index', 1],
        ['status', 'failed'],
        ['wall-time-ms', 60000],
        ['error', message],
      ]),
    ];
    for (const trial of list(raw['trials'])) {
      map(trial)['status'] = 'answered';
      map(trial)['score'] = null;
      map(trial)['error'] = message;
      trial['evaluations'] = [list(trial['evaluations'])[0]];
    }
    for (const person of participantsOf(raw)) {
      emptiedSummary(map(person['score']));
      person['baseline-difference'] = null;
      person['trials'] = structuredClone(
        list(raw['trials']).filter((trial) => trial['person-id'] === map(person['profile'])['id']),
      );
    }
    const decoded = decodeLegacySeriesResult(raw);
    expect(decoded.status).toBe('incomplete');
    expect(decoded.trials.every((trial) => trial.status === 'answered')).toBe(true);
    expect(decoded.trials.every((trial) => trial.score === null)).toBe(true);
    expect(decoded.trials.every((trial) => trial.attempt !== null)).toBe(true);
    expect(decoded.trials.every((trial) => trial.evaluations.length === 1)).toBe(true);
    expect(decoded.trials.every((trial) => trial.error === message)).toBe(true);
    expect(decoded.people[0].attempt!.answer).toBe(highSolveTraceAnswerA);
    expect(decoded.people[0].score.complete).toBe(false);
    expect(decoded.people[0].score.comparisonEligible).toBe(false);
    expect(decoded.comparison.difference).toBeNull();
    expect(decoded.comparison.comparisonEligible).toBe(false);
  });

  it('rejects incoherent lifecycle states', () => {
    const evaluatedWithoutScore = mutableClone(v2Fixture);
    list(evaluatedWithoutScore['trials'])[0]!['score'] = null;
    expectInvalid(evaluatedWithoutScore, 'evaluated trial');
    const skippedWithAttempt = mutableClone(v2Fixture);
    const trials = list(skippedWithAttempt['trials']);
    map(trials[1]!)['status'] = 'skipped';
    map(trials[1]!)['error'] = 'stopped';
    expectInvalid(skippedWithAttempt, 'skipped trial');
    const answeredWithScore = mutableClone(v2Fixture);
    map(list(answeredWithScore['trials'])[0]!)['status'] = 'answered';
    expectInvalid(answeredWithScore, "'answered' trial");
    const failedWithAttempt = mutableClone(v2Fixture);
    const failed = list(failedWithAttempt['trials'])[0]!;
    map(failed)['status'] = 'failed';
    map(failed)['attempt'] = null;
    map(failed)['error'] = 'boom';
    map(failed)['evaluations'] = [];
    expectInvalid(failedWithAttempt, "'failed' trial");
    const wrongId = mutableClone(v2Fixture);
    list(wrongId['trials'])[0]!['id'] = '2-a';
    expectInvalid(wrongId, "'id' must equal");
  });
});

describe('envelope', () => {
  it('rejects results above the 8 MiB serialized UTF-8 envelope', () => {
    const raw = mutableClone(v2Fixture);
    map(list(raw['people'])[0]!['artifacts'])['AGENTS.md'] = 'x'.repeat(8 * 1024 * 1024 + 1);
    expectInvalid(raw, 'exceeds the 8 MiB file envelope');
  });

  it('rejects non-JSON-serializable input', () => {
    const raw = mutableClone(v2Fixture);
    raw['self'] = raw;
    expectInvalid(raw, 'not JSON-serializable');
  });
});
