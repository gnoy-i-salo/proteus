import type { ExperimentResult } from '../experiment/result';
import { invalid } from './result-v4-validation';

function same(left: unknown, right: unknown, path: string): void {
  // Both sides have already been decoded into the same ordered schema. This
  // compares duplicated observations, never recomputes recorded scores.
  if (JSON.stringify(left) !== JSON.stringify(right)) invalid(path, 'conflicting saved copies');
}
function unique(ids: readonly string[], path: string): void {
  if (ids.some((id) => !id) || new Set(ids).size !== ids.length)
    invalid(path, 'expected distinct nonempty identifiers');
}

/** Validate references that the structured viewer uses to associate answers and scores. */
export function validateSeriesReferences(result: ExperimentResult): void {
  const participants = [...result.people, result.control];
  const ids = participants.map((person) =>
    result.schemaVersion === 5 ? person.participantId! : person.profile.id,
  );
  if (result.schemaVersion === 5 && ids.some((id, index) => id !== ['a', 'b', 'neutral'][index]))
    invalid('result.people', 'conflicting participant roles');
  unique(ids, 'result.people');
  if (result.schemaVersion === 5) {
    if (result.input.task !== result.protocol.task)
      invalid('result.protocol.task', 'task differs from series input');
    validateMemoryBindings(result);
  }

  const criteria = result.protocol.rubric.criteria.map((criterion) => criterion.id);
  unique(criteria, 'result.protocol.rubric.criteria');
  const assessments = (values: readonly { readonly criterionId: string }[], path: string) => {
    unique(
      values.map((item) => item.criterionId),
      path,
    );
    if (values.some((item) => !criteria.includes(item.criterionId)))
      invalid(path, 'unknown rubric criterion');
  };
  result.protocol.participantOrder.forEach((order, round) => {
    unique(order, `result.protocol.participantOrder[${round}]`);
    if (order.some((id) => !ids.includes(id)))
      invalid(`result.protocol.participantOrder[${round}]`, 'unknown participant');
  });
  unique(
    result.trials.map((trial) => trial.id),
    'result.trials',
  );
  result.trials.forEach((trial, index) => {
    const path = `result.trials[${index}]`;
    const round = Math.floor(index / 3) + 1;
    const personId = result.protocol.participantOrder[round - 1]![index % 3];
    if (trial.round !== round || trial.personId !== personId || trial.id !== `${round}-${personId}`)
      invalid(path, 'trial differs from the recorded participant order');
    if (trial.score) {
      if (trial.score.personId !== personId) invalid(`${path}.score`, 'wrong participant');
      assessments(trial.score.criteria, `${path}.score.criteria`);
      trial.score.criteria.forEach((criterion) => {
        if (criterion.assessments.some((item) => item.criterionId !== criterion.criterionId))
          invalid(`${path}.score.criteria`, 'assessment refers to a different criterion');
      });
    }
    trial.evaluations.forEach((evaluation) => {
      if (evaluation.personId !== personId) invalid(`${path}.evaluations`, 'wrong participant');
      assessments(evaluation.criteria, `${path}.evaluations.criteria`);
    });
  });
  participants.forEach((person, index) => {
    const path = index === 2 ? 'result.control' : `result.people[${index}]`;
    const id = ids[index]!;
    if (
      person.baseProfile.id !== person.profile.id ||
      !person.profile.id ||
      person.score.personId !== id
    )
      invalid(path, 'conflicting participant identity');
    const trials = result.trials.filter((trial) => trial.personId === id);
    same(person.trials, trials, `${path}.trials`);
    same(person.attempt, trials.find((trial) => trial.attempt)?.attempt ?? null, `${path}.attempt`);
  });
}

function validateMemoryBindings(result: ExperimentResult): void {
  const participants = [...result.people, result.control];
  const memory = result.protocol.memory;
  if (!memory) {
    if (
      participants.some(
        (person) => person.profile.memory || person.baseProfile.memory || person.taskAdaptation,
      )
    )
      invalid('result.protocol.memory', 'missing recorded memory bindings');
    return;
  }
  if (memory.participants.length !== 3)
    invalid('result.protocol.memory', 'expected three role bindings');
  unique(
    memory.participants.map((entry) => entry.participantId),
    'result.protocol.memory.participants',
  );
  for (const person of participants) {
    const binding = memory.participants.find(
      (entry) => entry.participantId === person.participantId,
    );
    if (
      !binding ||
      !person.profile.memory ||
      binding.manifestId !== person.profile.memory.manifestId
    )
      invalid('result.protocol.memory', 'profile differs from recorded manifest');
    same(person.baseProfile, person.profile, 'result.people.baseProfile');
    if (person.profile.taskSkills.length)
      invalid(
        'result.people.profile.taskSkills',
        'task guidance is separate from frozen memory profile',
      );
  }
}
