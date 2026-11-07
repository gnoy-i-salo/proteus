import type { ExperimentInput, ProviderId } from '@proteus-int/contracts';
import type { IntelligenceProvider, ModelSettings } from '@proteus-int/intelligence';
import { createRubric } from '@proteus-int/brain-analyzator';
import { createStageRunner } from '../execution/run-stage';
import { createExperimentSnapshot } from './input-snapshot';
import { buildTrialOrder, createProtocol, reviewProtocol } from './protocol';
import { preparePeople } from './prepare-people';
import { runTrials } from './run-trials';
import { buildExperimentResult } from './result';
import type { ExperimentResult } from './result';
import type { RunOptions } from './run-options';

export async function runExperiment(
  sourceInput: ExperimentInput,
  sourceSettings: ModelSettings,
  providers: Readonly<Record<ProviderId, IntelligenceProvider>>,
  options: RunOptions = {},
): Promise<ExperimentResult> {
  const { input, settings, sourceGrants, sourcePort, hasSelectedSources } =
    createExperimentSnapshot(sourceInput, sourceSettings, options);
  const repetitions = options.repetitions ?? 1;
  const order = buildTrialOrder(repetitions);
  const task = input.task;
  const total = 8 + repetitions * 5;
  const execution = createStageRunner(
    { settings, providers, sourceContext: hasSelectedSources, total },
    options,
  );
  const stage = execution.runStage;
  const generated = await stage('rubric', 'brain-analyzator', null, 'Критерии задачи', (provider) =>
    createRubric(task, provider, options.signal),
  );
  const protocol = createProtocol(
    task,
    settings,
    sourceGrants,
    repetitions,
    order,
    generated.rubric,
  );
  await reviewProtocol(protocol, options);
  const prepared = await preparePeople(input, sourceGrants, sourcePort, stage, options);
  const { trials, status } = await runTrials(task, protocol, prepared, stage, options);
  const result = buildExperimentResult(input, protocol, prepared, trials, status, execution.traces);
  result.people.forEach((person) => options.onPerson?.(person));
  options.onProgress?.({
    id: 'complete',
    stage: 'complete',
    person: null,
    completed: execution.completed,
    total,
    message: result.comparison.summary,
  });
  return result;
}
