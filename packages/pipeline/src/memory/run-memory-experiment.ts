import type { ExperimentInput, ExperimentProtocol, ProviderId } from '@proteus-int/contracts';
import type { IntelligenceProvider, ModelSettings } from '@proteus-int/intelligence';
import { componentIds } from '@proteus-int/intelligence';
import type { OpenPersonaMemory } from '@proteus-int/person';
import type { ExperimentResult } from '../experiment/result';
import type { RunOptions } from '../experiment/run-options';
import type { MemoryTrialsInput, MemoryTrialRun } from './run-memory-trials';
import { runMemoryTrials, selectMemoryTrials } from './run-memory-trials';
import { buildExperimentResult } from '../experiment/result';
import { freeze } from '../experiment/input-snapshot';
import { reviewProtocol } from '../experiment/protocol';
import { createStageRunner } from '../execution/run-stage';
import { validateExperiment } from '../validation';

export interface MemoryExperimentInput extends MemoryTrialsInput {
  readonly experiment: ExperimentInput;
}
export interface MemoryExperimentOptions extends Omit<
  RunOptions,
  'sourcePort' | 'sourceGrants' | 'repetitions' | 'onProtocol'
> {
  readonly onProtocol: (protocol: ExperimentProtocol) => Promise<void>;
}
export interface MemoryExperimentResult {
  readonly result: ExperimentResult;
  /** Live authorized data, excluded from ordinary stored/exported result. */
  readonly memoryRuns: readonly MemoryTrialRun[];
}
function protocolSettings(protocol: ExperimentProtocol): ModelSettings {
  const components = {} as Record<(typeof componentIds)[number], ProviderId>;
  const aliases = {} as Record<(typeof componentIds)[number], string>;
  for (const component of componentIds) {
    const model = protocol.modelSettings[component];
    const effort = component === 'stalker' || component === 'person-fabric' ? 'medium' : 'high';
    if (!model || model.requestedEffort !== effort)
      throw new Error('Настройки модели не соответствуют исполнению протокола.');
    components[component] = model.provider;
    aliases[component] = model.requestedModel ?? '';
  }
  return freeze({ globalModel: components.person, components, aliases });
}

/** Execute an already prepared rubric/protocol with saved, task-free personas. */
export async function runMemoryExperiment(
  input: MemoryExperimentInput,
  providers: Readonly<Record<ProviderId, IntelligenceProvider>>,
  open: OpenPersonaMemory,
  options: MemoryExperimentOptions,
): Promise<MemoryExperimentResult> {
  options.signal?.throwIfAborted();
  const selected = selectMemoryTrials(input);
  if (selected.protocol.memory!.neutralKnowledgeCondition !== 'unmatched')
    throw new Error(
      'Сопоставимый доступ к знаниям требует проверенных наблюдений контекста и поиска.',
    );
  const experiment = freeze(structuredClone(input.experiment));
  validateExperiment(experiment, [true, true]);
  if (
    typeof options.onProtocol !== 'function' ||
    experiment.task !== selected.task ||
    experiment.people.some(
      (person, index) => person.label !== selected.participants[index]!.profile.label,
    )
  )
    throw new Error('Условия задачи не соответствуют выбранным персонам и протоколу.');
  const settings = protocolSettings(selected.protocol);
  const selectedProviders = { codex: providers.codex, kimi: providers.kimi };
  for (const component of componentIds) {
    const id = settings.components[component];
    if (selectedProviders[id]?.id !== id) throw new Error('CLI не соответствует модели протокола.');
  }
  await reviewProtocol(selected.protocol, options);
  const total = selected.protocol.repetitions * 5;
  const execution = createStageRunner(
    { settings, providers: selectedProviders, sourceContext: true, total },
    options,
  );
  const outcome = await runMemoryTrials(selected, execution, open, options.signal);
  const result = {
    ...buildExperimentResult(
      experiment,
      selected.protocol,
      selected.participants,
      outcome.trials,
      outcome.status,
      execution.traces,
    ),
    execution: execution.snapshot(),
  };
  result.people.forEach((person) => options.onPerson?.(person));
  options.onProgress?.({
    id: 'complete',
    stage: 'complete',
    person: null,
    completed: execution.completed,
    total,
    message: result.comparison.summary,
  });
  return { result, memoryRuns: outcome.memoryRuns };
}
