import { describe, expect, it } from 'vitest';
import type { IntelligenceRequest, PersonaRule } from '@proteus-int/contracts';
import type { IntelligenceProvider } from '@proteus-int/intelligence';
import type { StalkerReport } from '@proteus-int/stalker';
import { adaptPerson, fabricatePerson, neutralProfile } from './index';

const input = { label: 'Ada', context: 'Checks assumptions', publicUrls: [] };
const rule: PersonaRule = {
  id: 'r1',
  instruction: 'Ada asks Boris before choosing; consult @boris at https://example.org/ada',
  evidenceIds: ['e1'],
  status: 'hypothesis',
  limitations: ['User description only'],
};
const report: StalkerReport = {
  evidence: [
    {
      id: 'e1',
      kind: 'hypothesis',
      summary: 'Ada checks assumptions',
      sourceUrl: '',
      status: 'hypothesis',
      verification: 'unverified',
      excerpt: null,
      retrievedAt: null,
      limitations: ['User description only'],
    },
  ],
  character: 'unknown',
  thinkingStyle: 'Checks assumptions',
  activity: 'unknown',
  behaviorSummary: 'unknown',
};
function provider(output: unknown, calls: IntelligenceRequest[] = []): IntelligenceProvider {
  return {
    id: 'codex',
    async run(request) {
      calls.push(request);
      return {
        answer: JSON.stringify(output),
        trace: { provider: 'codex', reasoningMs: 1, terminalMs: 0, summary: 'done' },
      };
    },
  };
}
describe('frozen evidence-linked personas', () => {
  it('keeps exact samples in a separate channel and links observed style to them', async () => {
    const sample = {
      id: 'sample-1',
      documentId: 'doc-1',
      mode: 'dialogue' as const,
      text: '  ну...\n\nда?! 🤷',
      context: '',
      redactions: [],
      exportAllowed: false,
    };
    const styleRule = {
      id: 'voice-1',
      instruction: 'Ada uses short replies to @boris',
      exemplarIds: ['sample-1'],
      status: 'observed',
      limitations: ['Small sample'],
    };
    const calls: IntelligenceRequest[] = [];
    const result = await fabricatePerson(
      { ...report, styleExemplars: [sample] },
      input,
      'a',
      'turquoise',
      provider(
        {
          rules: [rule],
          style: {
            reasoningPatterns: [],
            voiceRules: [styleRule],
            formatRules: [],
            limitations: [],
          },
        },
        calls,
      ),
    );
    expect(JSON.parse(calls[0]!.task).styleExemplars).toEqual([sample]);
    expect(result.profile.styleExemplars).toEqual([sample]);
    expect(result.profile.styleProfile?.voiceRules).toEqual([styleRule]);
    expect(result.profile.styleProfile?.contextModes).toContainEqual({
      mode: 'comment',
      status: 'insufficient',
      limitations: ['Нет собственных образцов для этого контекста.'],
    });
    await expect(
      fabricatePerson(
        { ...report, styleExemplars: [sample] },
        input,
        'a',
        'turquoise',
        provider({
          rules: [],
          style: {
            reasoningPatterns: [],
            voiceRules: [{ ...styleRule, exemplarIds: ['foreign'] }],
            formatRules: [],
            limitations: [],
          },
        }),
      ),
    ).rejects.toThrow(/exemplar/u);
  });
  it('keeps the original shared task identical for the neutral adaptation', async () => {
    const calls: IntelligenceRequest[] = [];
    const task = 'Нейтральный контроль нужен для сравнения. Design a room.';
    await adaptPerson(
      neutralProfile(),
      task,
      provider({ skills: [{ ...rule, id: 's1', evidenceIds: [] }] }, calls),
    );
    expect(JSON.parse(calls[0]!.task).task).toBe(task);
  });
  it('builds the profile without a task and preserves hypothesis status and opaque references', async () => {
    const calls: IntelligenceRequest[] = [];
    const { profile } = await fabricatePerson(
      report,
      input,
      'a',
      'turquoise',
      provider({ rules: [rule] }, calls),
    );
    expect(JSON.parse(calls[0]!.task)).not.toHaveProperty('task');
    expect(JSON.parse(calls[0]!.task)).toMatchObject({
      person: input,
      evidence: report.evidence,
    });
    expect(profile.rules).toEqual([rule]);
    expect(profile.sourceStatus).toBe('synthetic');
    expect(profile.skills).toEqual([]);
  });
  it('rejects invented references and upgrades from hypotheses into supported rules', async () => {
    await expect(
      fabricatePerson(
        report,
        input,
        'a',
        'turquoise',
        provider({ rules: [{ ...rule, evidenceIds: ['e404'] }] }),
      ),
    ).rejects.toThrow(/evidence/i);
    await expect(
      fabricatePerson(
        report,
        input,
        'a',
        'turquoise',
        provider({ rules: [{ ...rule, status: 'supported' }] }),
      ),
    ).rejects.toThrow(/supported/i);
  });
  it('keeps missing evidence as an explicit synthetic profile without fabricated traits', async () => {
    const { profile } = await fabricatePerson(
      { ...report, evidence: [] },
      input,
      'a',
      'turquoise',
      provider({ rules: [] }),
    );
    expect(profile.rules).toEqual([]);
    expect(profile.sourceStatus).toBe('synthetic');
  });
  it('adapts neutral and personal profiles under the same effort while freezing the archetype', async () => {
    const base = (
      await fabricatePerson(report, input, 'a', 'turquoise', provider({ rules: [rule] }))
    ).profile;
    const calls: IntelligenceRequest[] = [];
    const skill = { ...rule, id: 's1', instruction: 'List room requirements before the design' };
    const adapted = await adaptPerson(base, 'Design a room', provider({ skills: [skill] }, calls));
    const neutral = neutralProfile();
    await adaptPerson(
      neutral,
      'Design a room',
      provider({ skills: [{ ...skill, evidenceIds: [] }] }, calls),
    );
    expect(adapted.profile.archetype).toBe(base.archetype);
    expect(adapted.profile.rules).toEqual(base.rules);
    expect(base.skills).toEqual([]);
    expect(adapted.profile.skills).toEqual([skill.instruction]);
    expect(calls.map((call) => call.effort)).toEqual(['medium', 'medium']);
    expect(JSON.parse(calls[0]!.task).personaLabel).toBe('Ada');
    expect(JSON.parse(calls[1]!.task).personaLabel).toBeNull();
    expect(neutral.sourceStatus).toBe('neutral');
    expect(neutral.rules).toEqual([]);
  });
});

it('decodes legacy evidence and style fields into canonical rules with exact sample text', async () => {
  const sample = {
    id: 'sample-1',
    documentId: 'doc-1',
    mode: 'dialogue' as const,
    text: '  Анна → Борис: source-url\r\n👩🏽‍💻 ',
    context: '',
    redactions: [],
    exportAllowed: false,
  };
  const { evidenceIds, ...ruleFields } = rule;
  const styleRule = {
    id: 'voice-1',
    instruction: 'Short replies to Борис',
    'exemplar-ids': ['sample-1'],
    status: 'observed',
    limitations: [],
  };
  const result = await fabricatePerson(
    { ...report, styleExemplars: [sample] },
    input,
    'a',
    'turquoise',
    provider({
      rules: [{ ...ruleFields, 'evidence-ids': evidenceIds }],
      style: {
        'reasoning-patterns': [],
        'voice-rules': [styleRule],
        'format-rules': [],
        limitations: [],
      },
    }),
  );
  expect(result.profile.rules).toEqual([rule]);
  expect(result.profile.styleExemplars).toEqual([sample]);
  expect(result.profile.styleProfile?.version).toBe(2);
  expect(result.profile.styleProfile?.voiceRules[0]?.exemplarIds).toEqual(['sample-1']);
});

it('rejects mixed rule aliases in fabrication and adaptation', async () => {
  const mixed = { ...rule, 'evidence-ids': ['e1'] };
  await expect(
    fabricatePerson(report, input, 'a', 'turquoise', provider({ rules: [mixed] })),
  ).rejects.toThrow(/Mixed legacy and canonical/);
  const base = (await fabricatePerson(report, input, 'a', 'turquoise', provider({ rules: [rule] })))
    .profile;
  await expect(
    adaptPerson(base, 'Task', provider({ skills: [{ ...mixed, id: 's1' }] })),
  ).rejects.toThrow(/Mixed legacy and canonical/);
});

it('rejects mixed style field families instead of silently ignoring legacy fields', async () => {
  const sample = {
    id: 'sample-1',
    documentId: 'doc-1',
    mode: 'dialogue' as const,
    text: 'Exact source',
    context: '',
    redactions: [],
    exportAllowed: false,
  };
  await expect(
    fabricatePerson(
      { ...report, styleExemplars: [sample] },
      input,
      'a',
      'turquoise',
      provider({
        rules: [rule],
        style: {
          reasoningPatterns: [],
          voiceRules: [],
          formatRules: [],
          'voice-rules': [],
          limitations: [],
        },
      }),
    ),
  ).rejects.toThrow(/Mixed legacy and canonical/);
});
