import { expect, it } from 'vitest';
import v2 from '../testing/legacy-results/v2-kebab-result.json';
import v3 from '../testing/legacy-results/v3-kebab-export.json';
import early from '../testing/legacy-results/v4-camel-no-execution.json';
import current from '../testing/legacy-results/v4-current-with-execution.json';

it.each([v2, v3])(
  'keeps the legacy fixture base profile separate from task adaptation',
  (fixture) => {
    for (const person of fixture.people) {
      expect(person['base-profile'].skills).toEqual([]);
      expect(person['base-profile']['task-skills']).toEqual([]);
    }
    expect(fixture.control.profile['task-skills']).toHaveLength(1);
    expect(fixture.control.profile['task-skills'][0]?.['evidence-ids']).toEqual([]);
    expect(fixture.control.profile['task-skills'][0]?.status).toBe('hypothesis');
    expect(fixture.control.artifacts['.agents/skills/persona-1/SKILL.md']).toContain(
      'none (task strategy)',
    );
  },
);
it.each([early, current])(
  'keeps v4 fixture fabrication and reply copies writer-consistent',
  (fixture) => {
    for (const person of fixture.people) {
      expect(person.baseProfile.skills).toEqual([]);
      expect(person.baseProfile.taskSkills).toEqual([]);
    }
    expect(fixture.control.profile.taskSkills).toHaveLength(1);
    for (const person of [...fixture.people, fixture.control]) {
      expect(person.trials).toEqual(
        fixture.trials.filter((trial) => trial.personId === person.profile.id),
      );
      expect(person.attempt).toEqual(person.trials[0]?.attempt);
      expect(person.attempt.reply.version).toBe(2);
      expect(person.attempt.reply.messages.join('\n\n')).toBe(person.attempt.answer);
    }
  },
);
it('retains the v3 version1 reply after the original export filter', () => {
  for (const person of [...v3.people, v3.control]) {
    expect(person.trials).toEqual(
      v3.trials.filter((trial) => trial['person-id'] === person.profile.id),
    );
    expect(person.attempt).toEqual(person.trials[0]?.attempt);
    expect(person.attempt.reply.version).toBe(1);
    expect(person.attempt.reply.messages.join('\n\n')).toBe(person.attempt.answer);
  }
  expect(v3.people[0]!.attempt.reply.messages[0]).toContain(
    '[private source text omitted from export]',
  );
});
