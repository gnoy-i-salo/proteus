import { describe, expect, it } from 'vitest';
import type { IntelligenceRequest, PersonaRule } from '@proteus-int/contracts';
import type { IntelligenceProvider } from '@proteus-int/intelligence';
import type { StalkerReport } from '@proteus-int/stalker';
import { adaptPerson, fabricatePerson, neutralProfile } from './index';

const input = { label: 'Ada', context: 'Checks assumptions', 'public-urls': [] };
const rule: PersonaRule = {
  id: 'r1',
  instruction: 'Check assumptions before choosing',
  'evidence-ids': ['e1'],
  status: 'hypothesis',
  limitations: ['User description only'],
};
const report: StalkerReport = {
  evidence: [
    {
      id: 'e1',
      kind: 'hypothesis',
      summary: 'Ada checks assumptions',
      'source-url': '',
      status: 'hypothesis',
      verification: 'unverified',
      excerpt: null,
      'retrieved-at': null,
      limitations: ['User description only'],
    },
  ],
  character: 'unknown',
  'thinking-style': 'Checks assumptions',
  activity: 'unknown',
  'behavior-summary': 'unknown',
};
function provider(output: unknown, calls: IntelligenceRequest[] = []): IntelligenceProvider {
  return {
    id: 'codex',
    async run(request) {
      calls.push(request);
      return {
        answer: JSON.stringify(output),
        trace: { provider: 'codex', 'reasoning-ms': 1, 'terminal-ms': 0, summary: 'done' },
      };
    },
  };
}
describe('frozen evidence-linked personas', () => {
  it('keeps the already sanitized shared task identical for the neutral adaptation', async () => {
    const calls: IntelligenceRequest[] = [];
    const task = 'Нейтральный контроль нужен для сравнения. Design a room.';
    await adaptPerson(
      neutralProfile(),
      task,
      provider({ skills: [{ ...rule, id: 's1', 'evidence-ids': [] }] }, calls),
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
    expect(calls[0]!.task).not.toContain('Ada');
    expect(profile.rules).toEqual([rule]);
    expect(profile['source-status']).toBe('synthetic');
    expect(profile.skills).toEqual([]);
  });
  it('rejects invented references and upgrades from hypotheses into supported rules', async () => {
    await expect(
      fabricatePerson(
        report,
        input,
        'a',
        'turquoise',
        provider({ rules: [{ ...rule, 'evidence-ids': ['e404'] }] }),
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
    expect(profile['source-status']).toBe('synthetic');
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
      provider({ skills: [{ ...skill, 'evidence-ids': [] }] }, calls),
    );
    expect(adapted.profile.archetype).toBe(base.archetype);
    expect(adapted.profile.rules).toEqual(base.rules);
    expect(base.skills).toEqual([]);
    expect(adapted.profile.skills).toEqual([skill.instruction]);
    expect(calls.map((call) => call.effort)).toEqual(['medium', 'medium']);
    expect(calls.map((call) => call.task).join('')).not.toContain('Ada');
    expect(neutral['source-status']).toBe('neutral');
    expect(neutral.rules).toEqual([]);
  });
});
