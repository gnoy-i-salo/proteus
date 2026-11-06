import { expect, it } from 'vitest';
import type { IntelligenceRequest, PersonProfile } from '@proteus-int/contracts';
import type { IntelligenceProvider } from '@proteus-int/intelligence';
import { personaArtifacts, personRuntime } from './index';

const profile: PersonProfile = {
  id: 'a',
  label: 'Ada',
  palette: 'turquoise',
  archetype: 'Check assumptions',
  skills: ['List requirements'],
  'source-status': 'synthetic',
  rules: [
    {
      id: 'r1',
      instruction: 'Check assumptions',
      'evidence-ids': ['e1'],
      status: 'hypothesis',
      limitations: ['User description'],
    },
  ],
  'task-skills': [
    {
      id: 's1',
      instruction: 'List requirements',
      'evidence-ids': ['e1'],
      status: 'hypothesis',
      limitations: ['User description'],
    },
  ],
};
it('passes provenance limitations to the solver without personal identity', async () => {
  let request: IntelligenceRequest | undefined;
  const provider: IntelligenceProvider = {
    id: 'codex',
    async run(value) {
      request = value;
      return {
        answer: '{"answer":"A room plan","summary":"Plan complete"}',
        trace: { provider: 'codex', 'reasoning-ms': 1, 'terminal-ms': 0, summary: '' },
      };
    },
  };
  await personRuntime.solve(profile, 'Design a room', provider);
  expect(request!.task).not.toContain('Ada');
  expect(JSON.parse(request!.task).rules[0]).toMatchObject({
    status: 'hypothesis',
    'evidence-ids': ['e1'],
  });
});
it('exports the provenance and limitations alongside persona instructions', () => {
  const artifacts = personaArtifacts(profile);
  expect(artifacts['AGENTS.md']).toContain('synthetic');
  expect(artifacts['AGENTS.md']).toContain('e1');
  expect(artifacts['.agents/skills/persona-1/SKILL.md']).toContain('hypothesis');
  expect(artifacts['.agents/skills/persona-1/SKILL.md']).toContain('User description');
});
