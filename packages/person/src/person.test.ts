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
it('preserves exact Unicode, whitespace, Markdown and separate replies', async () => {
  const messages = ['  ну...\n\n  **да** 🤷🏽‍♀️  ', 'e\u0301\t\nок??\r\n'];
  const provider: IntelligenceProvider = {
    id: 'codex',
    async run(request) {
      expect(request.operation).toBe('persona-reply');
      expect(request['source-context']).toBe(true);
      return {
        answer: JSON.stringify({ messages, summary: 'Готово', 'style-conflicts': [] }),
        trace: { provider: 'codex', 'reasoning-ms': 1, 'terminal-ms': 0, summary: '' },
      };
    },
  };
  const result = await personRuntime.solve(
    { ...profile, 'source-context': true },
    'Ответь в диалоге',
    provider,
  );
  expect(result.reply).toEqual({ version: 1, messages, 'style-conflicts': [] });
  expect(result.answer).toBe(messages.join('\n\n'));
});
it('does not export sample text without separate export permission', () => {
  const artifacts = personaArtifacts({
    ...profile,
    'style-exemplars': [
      {
        id: 'sample-1',
        'document-id': 'document-1',
        mode: 'dialogue',
        text: 'не публиковать образец',
        context: '',
        redactions: [],
        'export-allowed': false,
      },
    ],
  });
  expect(JSON.stringify(artifacts)).not.toContain('не публиковать образец');
});
