import { expect, it } from 'vitest';
import { createModelSettings } from '@proteus-int/intelligence';
import type { IntelligenceProvider } from '@proteus-int/intelligence';
import { createStageRunner } from '../execution/run-stage';
import { runTrials } from './run-trials';
import { decodeV4Result } from '../history/result-v4';
import { readStoredResult } from '../history/read-stored-result';
import { buildExperimentResult } from './result';
import fixture from '../testing/legacy-results/v4-current-with-execution.json';

it('writes distinct series roles without renaming reusable persona identities', () => {
  const prior = decodeV4Result(fixture);
  const participants = [...prior.people, prior.control];
  const roles = ['a', 'b', 'neutral'] as const;
  const prepared = participants.map((person, index) => ({
    participantId: roles[index]!,
    report: person.report,
    base: { ...person.baseProfile, id: index === 2 ? 'saved-control' : 'shared-identity' },
    profile: { ...person.profile, id: index === 2 ? 'saved-control' : 'shared-identity' },
  }));
  const result = buildExperimentResult(
    prior.input,
    prior.protocol,
    prepared,
    prior.trials,
    prior.status,
    prior.traces,
  );
  expect(result.schemaVersion).toBe(5);
  expect(result.people.map((person) => person.participantId)).toEqual(['a', 'b']);
  expect(result.people.map((person) => person.profile.id)).toEqual([
    'shared-identity',
    'shared-identity',
  ]);
  expect(result.people.map((person) => person.trials)).toEqual(
    prior.people.map((person) => person.trials),
  );
  expect(result.people.map((person) => person.score)).toEqual(
    prior.people.map((person) => person.score),
  );
  expect(result.control.participantId).toBe('neutral');
  expect(readStoredResult(JSON.stringify(result))).toEqual({ sourceVersion: 5, result });
});

it('runs every role with its selected stable profile and preserves answered trials on judge failure', async () => {
  const prior = decodeV4Result(fixture);
  const roles = ['a', 'b', 'neutral'] as const;
  const prepared = [...prior.people, prior.control].map((person, index) => ({
    participantId: roles[index]!,
    report: person.report,
    base: { ...person.baseProfile, id: 'saved-identity' },
    profile: { ...person.profile, id: 'saved-identity', archetype: `Profile ${roles[index]}` },
  }));
  const seen: string[] = [];
  const provider: IntelligenceProvider = {
    id: 'codex',
    run(request) {
      if (request.systemPrompt.startsWith('BRAIN_ANALYZATOR'))
        return Promise.reject(new Error('Judge unavailable'));
      const payload = JSON.parse(request.task) as { archetype: string };
      seen.push(payload.archetype);
      return Promise.resolve({
        answer: JSON.stringify({ answer: payload.archetype, summary: 'Answer' }),
        trace: { provider: 'codex', reasoningMs: 0, terminalMs: 0, summary: '' },
      });
    },
  };
  const execution = createStageRunner(
    {
      settings: createModelSettings(),
      providers: { codex: provider, kimi: { ...provider, id: 'kimi' } },
      sourceContext: false,
      total: 5,
    },
    {},
  );
  const result = await runTrials(prior.input.task, prior.protocol, prepared, execution, {});
  expect(seen).toEqual(['Profile a', 'Profile b', 'Profile neutral']);
  expect(result.status).toBe('incomplete');
  expect(
    result.trials.map((trial) => [trial.personId, trial.status, trial.attempt?.answer]),
  ).toEqual([
    ['a', 'answered', 'Profile a'],
    ['b', 'answered', 'Profile b'],
    ['neutral', 'answered', 'Profile neutral'],
  ]);
  expect(
    execution.traces.filter((stage) => stage.stage === 'person').map((stage) => stage.person),
  ).toEqual([0, 1, 2]);
});
