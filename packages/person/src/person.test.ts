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
  sourceStatus: 'synthetic',
  rules: [
    {
      id: 'r1',
      instruction: 'Check assumptions',
      evidenceIds: ['e1'],
      status: 'hypothesis',
      limitations: ['User description'],
    },
  ],
  taskSkills: [
    {
      id: 's1',
      instruction: 'List requirements',
      evidenceIds: ['e1'],
      status: 'hypothesis',
      limitations: ['User description'],
    },
  ],
};
it('passes selected identity and first-person role instructions with provenance limitations', async () => {
  let request: IntelligenceRequest | undefined;
  const provider: IntelligenceProvider = {
    id: 'codex',
    async run(value) {
      request = value;
      return {
        answer: '{"answer":"A room plan","summary":"Plan complete"}',
        trace: { provider: 'codex', reasoningMs: 1, terminalMs: 0, summary: '' },
      };
    },
  };
  await personRuntime.solve(profile, 'Design a room', provider);
  expect(JSON.parse(request!.task).personaLabel).toBe('Ada');
  expect(request!.systemPrompt).toMatch(/first person/i);
  expect(JSON.parse(request!.task).rules[0]).toMatchObject({
    status: 'hypothesis',
    evidenceIds: ['e1'],
  });
});
it('exports the provenance and limitations alongside persona instructions', () => {
  const artifacts = personaArtifacts(profile);
  expect(artifacts['AGENTS.md']).toContain('synthetic');
  expect(artifacts['AGENTS.md']).toContain('e1');
  expect(artifacts['AGENTS.md']).toContain('Selected persona: "Ada"');
  expect(artifacts['.agents/skills/persona-1/SKILL.md']).toContain('hypothesis');
  expect(artifacts['.agents/skills/persona-1/SKILL.md']).toContain('User description');
});
it('keeps the neutral control outside the named first-person persona role', async () => {
  const neutral = {
    ...profile,
    label: 'Нейтральный контроль',
    sourceStatus: 'neutral' as const,
  };
  const provider: IntelligenceProvider = {
    id: 'codex',
    async run(request) {
      expect(JSON.parse(request.task).personaLabel).toBeNull();
      expect(request.systemPrompt).toContain('without adopting a personal identity');
      expect(request.systemPrompt).not.toContain('Respond in first person as the selected persona');
      return {
        answer: '{"messages":["A room plan"],"summary":"Plan complete"}',
        trace: { provider: 'codex', reasoningMs: 1, terminalMs: 0, summary: '' },
      };
    },
  };
  await personRuntime.solve(neutral, 'Design a room', provider);
  expect(personaArtifacts(neutral)['AGENTS.md']).not.toContain('Selected persona:');
});
it('preserves exact Unicode, whitespace, Markdown and separate replies', async () => {
  const messages = ['  ну...\n\n  **да** 🤷🏽‍♀️  ', 'e\u0301\t\nок??\r\n'];
  const provider: IntelligenceProvider = {
    id: 'codex',
    async run(request) {
      expect(request.operation).toBe('persona-reply');
      expect(request.sourceContext).toBe(true);
      return {
        answer: JSON.stringify({ messages, summary: 'Готово', styleConflicts: [] }),
        trace: { provider: 'codex', reasoningMs: 1, terminalMs: 0, summary: '' },
      };
    },
  };
  const result = await personRuntime.solve(
    { ...profile, sourceContext: true },
    'Ответь в диалоге',
    provider,
  );
  expect(result.reply).toEqual({ version: 2, messages, styleConflicts: [] });
  expect(result.answer).toBe(messages.join('\n\n'));
});
it('does not export sample text without separate export permission', () => {
  const artifacts = personaArtifacts({
    ...profile,
    styleExemplars: [
      {
        id: 'sample-1',
        documentId: 'document-1',
        mode: 'dialogue',
        text: 'не публиковать образец',
        context: '',
        redactions: [],
        exportAllowed: false,
      },
    ],
  });
  expect(JSON.stringify(artifacts)).not.toContain('не публиковать образец');
});

it.each(['legacy', 'canonical'])(
  'reads %s reply fields without changing message text',
  async (schema) => {
    const messages = ['  Анна: source-url\r\n**да** 👩🏽‍💻  '];
    const conflicts = ['Task asks a list'];
    const provider: IntelligenceProvider = {
      id: 'codex',
      async run() {
        return {
          answer: JSON.stringify({
            messages,
            summary: 'Готово',
            ...(schema === 'legacy'
              ? { 'style-conflicts': conflicts }
              : { styleConflicts: conflicts }),
          }),
          trace: { provider: 'codex', reasoningMs: 1, terminalMs: 0, summary: '' },
        };
      },
    };
    const result = await personRuntime.solve(profile, 'Исходная задача', provider);
    expect(result.reply).toEqual({ version: 2, messages, styleConflicts: conflicts });
    expect(result.answer).toBe(messages[0]);
  },
);

it('rejects mixed reply aliases instead of silently discarding a style conflict', async () => {
  const provider: IntelligenceProvider = {
    id: 'codex',
    async run() {
      return {
        answer: JSON.stringify({
          messages: ['Answer'],
          summary: 'Готово',
          styleConflicts: [],
          'style-conflicts': ['legacy conflict'],
        }),
        trace: { provider: 'codex', reasoningMs: 1, terminalMs: 0, summary: '' },
      };
    },
  };
  await expect(personRuntime.solve(profile, 'Task', provider)).rejects.toThrow(
    /Mixed legacy and canonical/,
  );
});

it('versions exported style artifacts and keeps permitted exemplars exact', () => {
  const sample = {
    id: 'sample-1',
    documentId: 'doc-1',
    mode: 'dialogue' as const,
    text: '  Анна → Борис\r\n**да** 👩🏽‍💻  ',
    context: 'Неизменный source-url',
    redactions: [],
    exportAllowed: true,
  };
  const styleProfile = {
    version: 2 as const,
    reasoningPatterns: [],
    voiceRules: [],
    formatRules: [],
    contextModes: [],
    styleExemplarIds: ['sample-1'],
    limitations: [],
  };
  const artifacts = personaArtifacts({
    ...profile,
    styleProfile,
    styleExemplars: [
      sample,
      { ...sample, id: 'private', text: 'Private source text', exportAllowed: false },
    ],
  });
  expect(JSON.parse(artifacts['style-profile.json']!)).toEqual(styleProfile);
  expect(JSON.parse(artifacts['style-exemplars.json']!)).toEqual({
    format: 'proteus-style-exemplars',
    version: 2,
    exemplars: [sample],
  });
  expect(artifacts['style-exemplars.json']).not.toContain('Private source text');
});
