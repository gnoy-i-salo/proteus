import { describe, expect, it } from 'vitest';
import type {
  IntelligenceRequest,
  PersonaRule,
  ProviderId,
  SourceDocument,
  SourceGrant,
} from '@proteus-int/contracts';
import { decodeExecutionSnapshot } from '@proteus-int/contracts';
import { createModelSettings } from '@proteus-int/intelligence';
import type { IntelligenceProvider } from '@proteus-int/intelligence';
import { exportExperiment, runExperiment, validateExperiment } from '../index';
import { decodeV4Result } from '../history/result-v4';

const input = {
  task: 'Design a public reading room',
  people: [
    { label: 'Ada', context: 'Careful planner', publicUrls: ['https://example.org/ada'] },
    { label: 'Boris', context: 'Improvises rapidly', publicUrls: [] },
  ],
} as const;
const rubric = [
  {
    id: 'coverage',
    label: 'Task coverage',
    weight: 1,
    required: false,
    anchors: { unmet: 'No solution', partial: 'Incomplete plan', met: 'Complete plan' },
  },
];
interface Call {
  readonly stage: string;
  readonly request: IntelligenceRequest;
  readonly provider: ProviderId;
}
function providers() {
  const calls: Call[] = [];
  const provider = (id: ProviderId): IntelligenceProvider => ({
    id,
    run(request, signal) {
      signal?.throwIfAborted();
      const stage = request.systemPrompt.split('\n')[0]!;
      calls.push({ stage, request, provider: id });
      const payload = JSON.parse(request.task) as Record<string, unknown>;
      let data: unknown;
      if (stage === 'BRAIN_ANALYZATOR_RUBRIC') data = { criteria: rubric };
      else if (request.operation === 'source-plan')
        data = { action: 'finish', reason: 'Available source read' };
      else if (request.operation === 'corpus-analysis')
        data = {
          evidence: [
            {
              id: 'e1',
              kind: 'fact',
              summary: 'Plans before acting',
              status: 'supported',
              documentIds: ['d1'],
              excerpt: 'Plans before acting',
              limitations: [],
            },
          ],
          character: 'unknown',
          thinkingStyle: 'unknown',
          activity: 'unknown',
          behaviorSummary: 'unknown',
        };
      else if (stage === 'STALKER')
        data = {
          evidence: [
            {
              id: 'e1',
              kind: 'hypothesis',
              sourceUrl: '',
              summary:
                payload.context === 'Careful planner'
                  ? 'Plans before acting'
                  : 'Tests ideas quickly',
              status: 'hypothesis',
              excerpt: null,
              limitations: ['User description only'],
            },
          ],
          character: 'unknown',
          thinkingStyle: 'unknown',
          activity: 'unknown',
          behaviorSummary: 'unknown',
        };
      else if (stage === 'PERSON_FABRIC') {
        const evidence = payload.evidence as { id: string; summary: string }[];
        data = {
          ...(payload.styleExemplars
            ? {
                style: {
                  reasoningPatterns: [],
                  voiceRules: [],
                  formatRules: [],
                  limitations: [],
                },
              }
            : {}),
          rules: [
            {
              id: 'r1',
              instruction: evidence[0]!.summary.startsWith('Plans')
                ? 'Plan first'
                : 'Prototype first',
              evidenceIds: ['e1'],
              status: 'hypothesis',
              limitations: [],
            },
          ],
        };
      } else if (stage === 'PERSON_FABRIC_ADAPT') {
        const rules = payload.rules as PersonaRule[];
        data = {
          skills: [
            {
              id: 's1',
              instruction: rules[0]?.instruction ?? 'Use a general plan',
              evidenceIds: rules[0]?.evidenceIds ?? [],
              status: 'hypothesis',
              limitations: [],
            },
          ],
        };
      } else if (stage === 'PERSON') {
        const rules = payload.rules as PersonaRule[];
        const answer = rules[0]?.instruction ?? 'Neutral plan';
        data = { answer, summary: 'A concrete plan' };
      } else if (stage === 'BRAIN_ANALYZATOR_EVALUATE') {
        const answers = payload.answers as { label: string; answer: string }[];
        data = {
          evaluations: answers.map((answer) => ({
            label: answer.label,
            criteria: [
              {
                criterionId: 'coverage',
                status: answer.answer === 'Plan first' ? 'met' : 'partial',
                reason: 'Observable completeness of the plan',
                quote: answer.answer,
              },
            ],
          })),
        };
      } else return Promise.reject(new Error(`Unexpected fixture stage: ${stage}`));
      return Promise.resolve({
        answer: JSON.stringify(data),
        trace: {
          provider: id,
          reasoningMs: 60000,
          wallTimeMs: 60000,
          terminalMs: 0,
          summary: 'Completed',
          reasoning: [{ id: 'r', kind: 'summary', text: 'Public summary' }],
        },
      });
    },
  });
  return { calls, adapters: { codex: provider('codex'), kimi: provider('kimi') } };
}
function payload(call: Call) {
  return JSON.parse(call.request.task) as Record<string, unknown>;
}

describe('controlled persona experiment', () => {
  const grant: SourceGrant = {
    id: 'source-grant',
    provider: 'telegram',
    accountId: 'local-account',
    resourceId: 'selected-chat',
    authorId: 'author-a',
    from: null,
    to: null,
    snapshotId: 'snapshot-a',
    expiresAt: '2099-01-01T00:00:00Z',
    allowModelProcessing: true,
    operations: ['collect', 'read', 'search', 'coverage', 'describe'],
    budget: { maxSteps: 20, maxDocuments: 50, maxBytes: 100000, maxDurationMs: 60000 },
  };
  const document: SourceDocument = {
    id: 'd1',
    provider: 'telegram',
    resourceId: 'selected-chat',
    authorId: 'author-a',
    authorLabel: 'Ada',
    authorship: 'owner',
    kind: 'message',
    mode: 'dialogue',
    text: 'Plans before acting',
    sourceUrl: null,
    createdAt: '2026-01-01T00:00:00Z',
    retrievedAt: '2026-02-01T00:00:00Z',
    contentHash: 'hash-one',
    replyTo: null,
    isForwarded: false,
    truncated: false,
    partition: 'reference',
  };
  it('uses a frozen selected source without web fallback and keeps successive source calls accepted', async () => {
    const { calls, adapters } = providers();
    const selected = structuredClone(grant);
    const executions: string[] = [];
    const result = await runExperiment(input, createModelSettings(), adapters, {
      sourceGrants: [[selected], []],
      sourcePort: {
        async reserveModelCall() {},
        execute(id) {
          executions.push(id);
          return Promise.resolve({
            snapshotId: 'snapshot-a',
            documents: [document],
            cursor: null,
            coverage: {
              documents: 1,
              totalDocuments: 1,
              bytes: 19,
              complete: true,
              truncated: false,
              notAnalyzed: 0,
              unavailable: [],
            },
            limitations: [],
          });
        },
      },
      onProtocol: (protocol) => {
        expect(protocol.sourceSnapshots).toEqual([['snapshot-a'], []]);
        Object.assign(selected, { id: 'mutated-grant' });

        return Promise.resolve();
      },
    });
    expect(executions).toEqual(['source-grant']);
    expect(calls.filter((call) => call.request.operation === 'public-research')).toHaveLength(1);
    expect(calls.filter((call) => call.request.operation === 'source-plan')).toHaveLength(1);
    expect(calls.filter((call) => call.request.operation === 'corpus-analysis')).toHaveLength(1);
    expect(calls.every((call) => call.request.sourceContext === true)).toBe(true);
    expect(result.people[0]?.report?.evidence[0]?.verification).toBe('source-retrieved');
    expect(
      result.traces.find((item) => item.id === 'sources-0')?.attempts.map((item) => item.status),
    ).toEqual(['accepted', 'accepted']);
    expect(JSON.stringify(result)).not.toMatch(
      /selected-chat|local-account|source-grant|mutated-grant/,
    );
  });
  it('rejects a selected source without its port or processing permission before model calls', async () => {
    const { calls, adapters } = providers();
    await expect(
      runExperiment(input, createModelSettings(), adapters, { sourceGrants: [[grant], []] }),
    ).rejects.toThrow(/source|источник/i);
    await expect(
      runExperiment(input, createModelSettings(), adapters, {
        sourceGrants: [[{ ...grant, allowModelProcessing: false }], []],
        sourcePort: {
          async reserveModelCall() {},
          execute() {
            return Promise.reject(new Error('Should not retrieve'));
          },
        },
      }),
    ).rejects.toThrow(/grant|источник/i);
    expect(calls).toEqual([]);
  });
  it('exports only permitted style samples without changing the in-memory frozen profile', async () => {
    const { adapters } = providers();
    const result = await runExperiment(input, createModelSettings(), adapters);
    const privateSample = {
      id: 's1',
      documentId: 'd1',
      mode: 'dialogue' as const,
      text: 'PRIVATE ORIGINAL STYLE SAMPLE',
      context: 'PRIVATE NEIGHBORING CONTEXT',
      redactions: [],
      exportAllowed: false,
    };
    const allowed = {
      ...privateSample,
      id: 's2',
      text: 'Permitted public example',
      context: 'Public context',
      exportAllowed: true,
    };
    const person = result.people[0]!;
    const exampleResult = {
      ...result,
      people: [
        {
          ...person,
          report: { ...person.report!, styleExemplars: [privateSample, allowed] },
          profile: { ...person.profile, styleExemplars: [privateSample, allowed] },
          baseProfile: {
            ...person.baseProfile,
            styleExemplars: [privateSample, allowed],
          },
          artifacts: {
            ...person.artifacts,
            'style-exemplars.json': JSON.stringify([privateSample, allowed]),
          },
        },
        result.people[1]!,
      ],
    };
    const exported = exportExperiment(exampleResult);
    expect(exported).not.toMatch(/PRIVATE ORIGINAL|PRIVATE NEIGHBORING/);
    expect(exported).toContain('Permitted public example');
    expect(exampleResult.people[0]!.profile.styleExemplars).toHaveLength(2);
  });
  it('omits every connected-source excerpt including short and non-exemplar quotes', async () => {
    const { adapters } = providers();
    const result = await runExperiment(input, createModelSettings(), adapters);
    const original = result.people[0]!;
    const report = {
      ...original.report!,
      evidence: [
        {
          ...original.report!.evidence[0]!,
          id: 'e1',
          verification: 'source-retrieved' as const,
          excerpt: 'ok',
        },
        {
          ...original.report!.evidence[0]!,
          id: 'e2',
          verification: 'source-retrieved' as const,
          excerpt: 'PRIVATE SOURCE QUOTE WITHOUT STYLE SAMPLE',
        },
      ],
    };
    const withQuotes = { ...result, people: [{ ...original, report }, result.people[1]!] };
    const exported = JSON.parse(exportExperiment(withQuotes)) as typeof withQuotes & {
      exportMetadata: {
        redacted: boolean;
        omittedSourceExcerpts: number;
        preparationReasoningOmitted: boolean;
      };
    };
    expect(exported.people[0]!.report!.evidence.map((item) => item.excerpt)).toEqual([null, null]);
    expect(exported.exportMetadata).toMatchObject({
      redacted: true,
      omittedSourceExcerpts: 2,
      preparationReasoningOmitted: true,
    });
    expect(report.evidence.map((item) => item.excerpt)).toEqual([
      'ok',
      'PRIVATE SOURCE QUOTE WITHOUT STYLE SAMPLE',
    ]);
  });
  it('freezes task-only criteria before reading either personal profile', async () => {
    const requests: IntelligenceRequest[] = [];
    const adapter: IntelligenceProvider = {
      id: 'codex',
      run(request) {
        requests.push(request);
        return Promise.reject(new Error('Stop after observing the first stage'));
      },
    };
    await expect(
      runExperiment(input, createModelSettings(), { codex: adapter, kimi: adapter }),
    ).rejects.toThrow('Stop after observing');
    expect(requests[0]?.systemPrompt).toMatch(/^BRAIN_ANALYZATOR_RUBRIC/);
    expect(JSON.parse(requests[0]!.task)).toEqual({ task: input.task });
  });

  it('awaits a frozen protocol, routes components and compares against a real neutral attempt', async () => {
    const { calls, adapters } = providers();
    const settings = createModelSettings();
    let reviewed = false;
    const stages: string[] = [];
    const original = adapters.kimi.run.bind(adapters.kimi);
    adapters.kimi.run = async (request, signal) => {
      expect(reviewed).toBe(true);
      return original(request, signal);
    };
    const result = await runExperiment(
      input,
      {
        ...settings,
        components: { ...settings.components, stalker: 'kimi' },
      },
      adapters,
      {
        onProtocol: async (protocol) => {
          expect(calls.map((call) => call.stage)).toEqual(['BRAIN_ANALYZATOR_RUBRIC']);
          expect(Object.isFrozen(protocol)).toBe(true);
          expect(Object.isFrozen(protocol.rubric.criteria[0]?.anchors)).toBe(true);
          await Promise.resolve();
          reviewed = true;
        },
        onProgress: (event) => stages.push(event.stage),
      },
    );
    expect(result).toMatchObject({ schemaVersion: 4, scoringVersion: 2, status: 'complete' });
    expect(calls).toHaveLength(13);
    expect(result.traces).toHaveLength(13);
    expect(calls.filter((call) => call.provider === 'kimi')).toHaveLength(2);
    expect(result.people.map((person) => person.score.point)).toEqual([100, 50]);
    expect(result.control.attempt?.answer).toBe('Neutral plan');
    expect(result.control.score.point).toBe(50);
    expect(result.control.report).toBeNull();
    expect(result.control.profile.sourceStatus).toBe('neutral');
    expect(result.people[0]?.baselineDifference).toEqual({ lower: 50, upper: 50 });
    expect(result.comparison.comparisonEligible).toBe(false);
    expect(result.trials.map((trial) => trial.status)).toEqual([
      'evaluated',
      'evaluated',
      'evaluated',
    ]);
    expect(result.trials.every((trial) => trial.evaluations.length === 2)).toBe(true);
    expect(stages.at(-1)).toBe('complete');
    expect(result.traces.every((trace) => trace.attempts.at(-1)?.status === 'accepted')).toBe(true);
  });

  it('shares the original task and preserves identity while keeping profile construction task-free', async () => {
    const { calls, adapters } = providers();
    const taskInput = {
      ...input,
      task: 'Ada and Boris design a room using https://standards.example/spec',
    };
    const result = await runExperiment(taskInput, createModelSettings(), adapters);
    expect(result.protocol.task).toBe(taskInput.task);
    expect(result.protocol.promptVersion).toBe('bsc-v2.4-camel-case');
    expect(result.protocol.task).toContain('https://standards.example/spec');
    for (const call of calls) {
      const data = payload(call);
      if (call.stage === 'STALKER' || call.stage === 'PERSON_FABRIC')
        expect(data).not.toHaveProperty('task');
      else expect(data.task).toBe(result.protocol.task);
      if (call.stage === 'PERSON_FABRIC') {
        expect(['Ada', 'Boris']).toContain((data.person as { label: string }).label);
        expect(data.evidence).toMatchObject([{ id: 'e1', status: 'hypothesis' }]);
      }
    }
    expect(result.people.every((person) => person.profile.sourceStatus === 'synthetic')).toBe(true);
    expect(result.people[0]?.profile.rules[0]?.limitations).toContain('User description only');
    expect(result.people[0]?.profile.taskSkills[0]?.limitations).toContain('User description only');
    expect(result.people[0]?.artifacts['AGENTS.md']).toContain('e1');
  });

  it('freezes profiles and model settings while balancing order across three rounds', async () => {
    const { calls, adapters } = providers();
    const settings = {
      ...createModelSettings(),
      aliases: { ...createModelSettings().aliases, person: 'fixed-model' },
    };
    const result = await runExperiment(input, settings, adapters, {
      repetitions: 3,
      onProtocol: () => {
        settings.aliases.person = 'later-change';

        return Promise.resolve();
      },
    });
    expect(calls).toHaveLength(23);
    expect(calls.filter((call) => call.stage === 'PERSON_FABRIC')).toHaveLength(2);
    const adaptations = calls.filter((call) => call.stage === 'PERSON_FABRIC_ADAPT');
    expect(adaptations).toHaveLength(3);
    expect(adaptations.map((call) => call.request.effort)).toEqual(['medium', 'medium', 'medium']);
    expect(result.trials.map((trial) => trial.personId)).toEqual([
      'a',
      'b',
      'neutral',
      'b',
      'neutral',
      'a',
      'neutral',
      'a',
      'b',
    ]);
    const solving = calls.filter((call) => call.stage === 'PERSON');
    expect(
      solving.every(
        (call) => call.request.model === 'fixed-model' && call.request.effort === 'high',
      ),
    ).toBe(true);
    const uniquePrompts = new Set(solving.map((call) => call.request.task));
    expect(uniquePrompts.size).toBe(3);
    for (const prompt of uniquePrompts)
      expect(solving.filter((call) => call.request.task === prompt)).toHaveLength(3);
    expect(result.people.every((person) => Object.isFrozen(person.profile))).toBe(true);
    expect(
      result.people.every((person) => person.profile.archetype === person.baseProfile.archetype),
    ).toBe(true);
    expect(
      result.trials.every((trial) => trial.score?.interval.lower === trial.score?.interval.upper),
    ).toBe(true);
  });

  it('judges answer-only payloads twice in opposite orders without persona metadata', async () => {
    const { calls, adapters } = providers();
    await runExperiment(input, createModelSettings(), adapters);
    const judges = calls.filter((call) => call.stage === 'BRAIN_ANALYZATOR_EVALUATE').map(payload);
    const answers = judges.map((judge) => judge.answers as { label: string; answer: string }[]);
    expect(answers[1]?.map((answer) => answer.answer)).toEqual(
      [...answers[0]!].reverse().map((answer) => answer.answer),
    );
    for (const judge of judges) {
      expect(Object.keys(judge).sort()).toEqual(['answers', 'rubric', 'task']);
      expect(JSON.stringify(judge)).not.toMatch(
        /Ada|Boris|"trace"|"provider"|"archetype"|"score"|"person-id"/,
      );
      for (const answer of judge.answers as object[])
        expect(Object.keys(answer).sort()).toEqual(['answer', 'label']);
    }
  });

  it('repairs one PERSON format error without repeating preparation or dropping time and traces', async () => {
    const { calls, adapters } = providers();
    const original = adapters.codex.run.bind(adapters.codex);
    let malformed = false;
    adapters.codex.run = async (request, signal) => {
      const result = await original(request, signal);
      if (!malformed && request.systemPrompt.startsWith('PERSON\n')) {
        malformed = true;
        return { ...result, answer: '{"answer":{},"summary":"bad format"}' };
      }
      return result;
    };
    const result = await runExperiment(input, createModelSettings(), adapters);
    expect(result.status).toBe('complete');
    expect(calls).toHaveLength(14);
    expect(result.people[0]?.attempt?.trace.wallTimeMs).toBe(120000);
    expect(result.people[0]?.attempt?.trace.reasoning?.map((block) => block.id)).toEqual([
      '1:r',
      '2:r',
    ]);
    expect(
      result.traces
        .find((trace) => trace.id === 'solve-1-a')
        ?.attempts.map((attempt) => attempt.status),
    ).toEqual(['format-rejected', 'accepted']);
    expect(calls.filter((call) => call.stage === 'STALKER')).toHaveLength(2);
  });

  it('returns partial trials and stops further calls after a CLI failure during solving', async () => {
    const { calls, adapters } = providers();
    const original = adapters.codex.run.bind(adapters.codex);
    let solving = 0;
    adapters.codex.run = async (request, signal) => {
      const result = await original(request, signal);
      if (request.systemPrompt.startsWith('PERSON\n') && ++solving === 2)
        throw new Error('CLI unavailable');
      return result;
    };
    const result = await runExperiment(input, createModelSettings(), adapters, { repetitions: 3 });
    expect(result.status).toBe('incomplete');
    expect(calls).toHaveLength(10);
    expect(result.trials[0]).toMatchObject({ status: 'answered', score: null });
    expect(result.trials[1]).toMatchObject({ status: 'failed', error: 'CLI unavailable' });
    expect(result.trials.slice(2).every((trial) => trial.status === 'skipped')).toBe(true);
    expect(result.people[0]?.attempt?.answer).toBe('Plan first');
    expect(result.control.attempt).toBeNull();
    expect(result.comparison).toMatchObject({ difference: null, comparisonEligible: false });
    expect(result.traces.at(-1)?.attempts.at(-1)?.status).toBe('failed');
  });

  it('retains first-pass judgments without inventing scores when the second judge fails', async () => {
    const { calls, adapters } = providers();
    const original = adapters.codex.run.bind(adapters.codex);
    let judging = 0;
    adapters.codex.run = async (request, signal) => {
      const result = await original(request, signal);
      if (request.systemPrompt.startsWith('BRAIN_ANALYZATOR_EVALUATE\n') && ++judging === 2)
        throw new Error('Judge failed');
      return result;
    };
    const result = await runExperiment(input, createModelSettings(), adapters);
    expect(result.status).toBe('incomplete');
    expect(calls).toHaveLength(13);
    expect(
      result.trials.every((trial) => trial.evaluations.length === 1 && trial.score === null),
    ).toBe(true);
    expect(result.people.every((person) => person.score.point === null)).toBe(true);
  });

  it('cancels after an accepted solution and keeps its answer without calling later stages', async () => {
    const { calls, adapters } = providers();
    const controller = new AbortController();
    const result = await runExperiment(input, createModelSettings(), adapters, {
      signal: controller.signal,
      onProgress: (event) => {
        if (event.id === 'solve-1-a' && event.message.endsWith('готово')) controller.abort();
      },
    });
    expect(result.status).toBe('cancelled');
    expect(calls).toHaveLength(9);
    expect(result.people[0]?.attempt?.answer).toBe('Plan first');
    expect(result.control.attempt).toBeNull();
    expect(result.comparison.comparisonEligible).toBe(false);
  });

  it('stops before source collection if saving/reviewing the protocol fails', async () => {
    const { calls, adapters } = providers();
    await expect(
      runExperiment(input, createModelSettings(), adapters, {
        onProtocol: () => {
          return Promise.reject(new Error('Protocol not accepted'));
        },
      }),
    ).rejects.toThrow('Protocol not accepted');
    expect(calls.map((call) => call.stage)).toEqual(['BRAIN_ANALYZATOR_RUBRIC']);
  });

  it.each([
    'http://localhost/private',
    'http://127.0.0.1',
    'https://192.168.1.2',
    'file:///etc/passwd',
    'https://user:secret@example.com',
    'https://[::1]',
  ])('rejects non-public URL %s before contacting a provider', async (url) => {
    const { calls, adapters } = providers();
    await expect(
      runExperiment(
        { ...input, people: [{ ...input.people[0], publicUrls: [url] }, input.people[1]] },
        createModelSettings(),
        adapters,
      ),
    ).rejects.toThrow();
    expect(calls).toEqual([]);
  });

  it('requires a task and two described people', () => {
    expect(() => validateExperiment({ ...input, task: ' ' })).toThrow();
    expect(() =>
      validateExperiment({
        ...input,
        people: [{ label: 'A', context: '', publicUrls: [] }, input.people[1]],
      }),
    ).toThrow();
  });
});

it('exports versioned lifecycle observations and rejects corrupted metadata', async () => {
  const { adapters } = providers();
  const result = await runExperiment(input, createModelSettings(), adapters);
  const exported = JSON.parse(exportExperiment(result)) as Record<string, unknown>;
  expect(decodeExecutionSnapshot(exported.execution)).toEqual(result.execution);
  expect(result.execution?.stages).toHaveLength(13);
  expect(result.execution?.stages.every((stage) => stage.status === 'completed')).toBe(true);
  const legacy = { ...result };
  delete legacy.execution;
  expect(
    decodeExecutionSnapshot(
      (JSON.parse(exportExperiment(legacy)) as Record<string, unknown>).execution,
    ),
  ).toBeNull();
  const corrupted = structuredClone(result);
  Object.assign(corrupted.execution!, { schemaVersion: 99 });
  expect(() => exportExperiment(corrupted)).toThrow(/execution metadata/u);
});

it.each(['complete', 'incomplete', 'cancelled'] as const)(
  'reads actual %s series and redacted exports without changing scores or observations',
  async (status) => {
    const { adapters, calls } = providers();
    const controller = new AbortController();
    const original = adapters.codex.run.bind(adapters.codex);
    let solutions = 0;
    adapters.codex.run = async (request, signal) => {
      if (
        status === 'incomplete' &&
        request.systemPrompt.startsWith('PERSON\n') &&
        ++solutions === 2
      )
        throw new Error('Fixture provider unavailable');
      return original(request, signal);
    };
    const result = await runExperiment(input, createModelSettings(), adapters, {
      repetitions: 3,
      signal: controller.signal,
      onProgress: (event) => {
        if (status === 'cancelled' && event.id === 'solve-1-a' && event.message.endsWith('готово'))
          controller.abort();
      },
    });
    expect(result.status).toBe(status);
    const before = JSON.stringify(result);
    const callCount = calls.length;
    expect(decodeV4Result(JSON.parse(before))).toEqual(JSON.parse(before));
    const exported = JSON.parse(exportExperiment(result)) as unknown;
    expect(decodeV4Result(exported)).toEqual(exported);
    expect(JSON.stringify(result)).toBe(before);
    expect(calls).toHaveLength(callCount);
  },
);
