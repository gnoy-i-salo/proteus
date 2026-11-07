import type { ExperimentProtocol } from '@proteus-int/contracts';
import { personRuntime } from '@proteus-int/person';
import { scoreTrial } from '@proteus-int/brain-scoring';
import { evaluateAnswers } from '@proteus-int/brain-analyzator';
import type { createStageRunner } from '../execution/run-stage';
import type { PreparedPerson } from './prepare-people';
import type { TrialResult, ExperimentResult } from './result';
import type { RunOptions } from './run-options';

export async function runTrials(
  task: string,
  protocol: ExperimentProtocol,
  prepared: readonly PreparedPerson[],
  execution: ReturnType<typeof createStageRunner>,
  options: Pick<RunOptions, 'signal'>,
) {
  const { repetitions, participantOrder: order } = protocol;
  const stage = execution.runStage;
  const trials: TrialResult[] = order.flatMap((ids, round) =>
    ids.map((id) => ({
      id: `${round + 1}-${id}`,
      round: round + 1,
      personId: id,
      status: 'pending',
      attempt: null,
      score: null,
      evaluations: [],
      error: null,
    })),
  );
  let status: ExperimentResult['status'] = 'complete';
  let activeTrial: TrialResult | undefined;
  const replace = (next: TrialResult) => {
    trials[trials.findIndex((item) => item.id === next.id)] = next;
  };
  try {
    for (let round = 1; round <= repetitions; round++) {
      for (const planned of trials.filter((item) => item.round === round)) {
        activeTrial = planned;
        const index = prepared.findIndex((person) => person.profile.id === planned.personId);
        const participant = prepared[index]!;
        const attempt = await stage(
          `solve-${planned.id}`,
          'person',
          index,
          `Решение · прогон ${round}`,
          (provider) => personRuntime.solve(participant.profile, task, provider, options.signal),
        );
        replace({ ...planned, status: 'answered', attempt });
        activeTrial = undefined;
      }
      const answers = trials
        .filter((item) => item.round === round)
        .map((item) => ({
          id: item.personId,
          answer: item.attempt!.answer,
        }));
      for (const [pass, ordered] of [answers, [...answers].reverse()].entries()) {
        const { evaluations } = await stage(
          `judge-${round}-${pass}`,
          'brain-analyzator',
          null,
          `Слепая оценка · прогон ${round} · порядок ${pass + 1}`,
          (provider) => evaluateAnswers(task, protocol.rubric, ordered, provider, options.signal),
        );
        for (const evaluation of evaluations) {
          const trial = trials.find(
            (item) => item.round === round && item.personId === evaluation.personId,
          )!;
          replace({
            ...trial,
            evaluations: [...trial.evaluations, evaluation],
          });
        }
      }
      for (const trial of trials.filter((item) => item.round === round)) {
        replace({
          ...trial,
          status: 'evaluated',
          score: scoreTrial(trial.personId, protocol.rubric, trial.evaluations),
        });
      }
    }
  } catch (failure) {
    status = options.signal?.aborted ? 'cancelled' : 'incomplete';
    const message = failure instanceof Error ? failure.message : String(failure);
    if (activeTrial) replace({ ...activeTrial, status: 'failed', error: message });
    for (const trial of trials) {
      if (trial.status === 'pending') {
        replace({ ...trial, status: 'skipped', error: message });
        const index = prepared.findIndex((person) => person.profile.id === trial.personId);
        execution.skipStage(
          `solve-${trial.id}`,
          'person',
          index,
          `Решение · прогон ${trial.round}`,
          message,
        );
      } else if (trial.status === 'answered') replace({ ...trial, error: message });
    }
    for (let round = 1; round <= repetitions; round++) {
      for (let pass = 0; pass < 2; pass++) {
        execution.skipStage(
          `judge-${round}-${pass}`,
          'brain-analyzator',
          null,
          `Слепая оценка · прогон ${round} · порядок ${pass + 1}`,
          message,
        );
      }
    }
  }
  return { trials, status };
}
