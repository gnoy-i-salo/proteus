import type { ExperimentInput, SourceGrant } from '@proteus-int/contracts';
import type { ModelSettings } from '@proteus-int/intelligence';
import { validateSourceGrant } from '@proteus-int/stalker';
import { validateExperiment } from '../validation';
import type { RunOptions } from './run-options';

export function freeze<T>(value: T): T {
  if (value && typeof value === 'object') {
    Object.values(value).forEach(freeze);
    Object.freeze(value);
  }
  return value;
}

export function createExperimentSnapshot(
  sourceInput: ExperimentInput,
  sourceSettings: ModelSettings,
  options: Pick<RunOptions, 'sourcePort' | 'sourceGrants'>,
) {
  const input = freeze(structuredClone(sourceInput));
  const settings = freeze(structuredClone(sourceSettings));
  const sourceGrants = freeze(structuredClone(options.sourceGrants ?? [[], []])) as readonly [
    readonly SourceGrant[],
    readonly SourceGrant[],
  ];
  const sourcePort = options.sourcePort;
  if (
    !Array.isArray(sourceGrants) ||
    sourceGrants.length !== 2 ||
    sourceGrants.some((grants) => !Array.isArray(grants) || grants.length > 5)
  )
    throw new Error('Выберите до пяти источников на каждую из двух персон.');
  const selected = sourceGrants.flat();
  if (selected.length && !sourcePort)
    throw new Error('Для выбранных источников не настроен source port.');
  if (new Set(selected.map((grant) => grant.id)).size !== selected.length)
    throw new Error('Один source grant нельзя повторно использовать в одном эксперименте.');
  selected.forEach(validateSourceGrant);
  validateExperiment(input, [sourceGrants[0].length > 0, sourceGrants[1].length > 0]);
  return { input, settings, sourceGrants, sourcePort, hasSelectedSources: selected.length > 0 };
}
