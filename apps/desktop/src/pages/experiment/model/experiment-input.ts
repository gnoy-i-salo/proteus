import { batch, createSignal } from 'solid-js';
import type { ExperimentInput, PersonInput } from '@proteus-int/contracts';

export function createExperimentInput() {
  const [revision, setRevision] = createSignal(0);
  const [names, setNames] = createSignal(['Person A', 'Person B']);
  const [urls, setUrls] = createSignal<string[][]>([[''], ['']]);
  const [contexts, setContexts] = createSignal<string[][]>([[''], ['']]);
  const [task, setTask] = createSignal(['']);
  const [repetitions, setRepetitions] = createSignal<1 | 3>(1);
  const currentInput = (): ExperimentInput => {
    const person = (index: number): PersonInput => ({
      label: names()[index] ?? '',
      context: contexts()[index]?.join('') ?? '',
      publicUrls: (urls()[index] ?? []).map((url) => url.trim()).filter(Boolean),
    });
    return { task: task().join(''), people: [person(0), person(1)] };
  };
  const applyInput = (input: ExperimentInput) =>
    batch(() => {
      setNames(input.people.map((person) => person.label));
      setUrls(
        input.people.map((person) => (person.publicUrls.length ? [...person.publicUrls] : [''])),
      );
      setContexts(input.people.map((person) => [person.context]));
      setTask([input.task]);
      setRevision((value) => value + 1);
    });
  return {
    revision,
    names,
    urls,
    contexts,
    task,
    repetitions,
    currentInput,
    applyInput,
    setNames,
    setUrls,
    setContexts,
    setTask,
    setRepetitions,
  };
}
export type ExperimentInputModel = ReturnType<typeof createExperimentInput>;
