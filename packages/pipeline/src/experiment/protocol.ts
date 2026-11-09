import type { EvaluationRubric, ExperimentProtocol, SourceGrant } from '@proteus-int/contracts';
import { componentIds } from '@proteus-int/intelligence';
import type { ModelSettings } from '@proteus-int/intelligence';
import { freeze } from './input-snapshot';
import type { RunOptions } from './run-options';

export const protocolPromptVersion = 'bsc-v2.4-camel-case';

export function buildTrialOrder(repetitions: 1 | 3): readonly (readonly string[])[] {
  if (repetitions !== 1 && repetitions !== 3) throw new Error('Нужен 1 или 3 прогона.');
  const ids = ['a', 'b', 'neutral'];
  return Array.from({ length: repetitions }, (_, round) =>
    ids.map((_, offset) => ids[(round + offset) % ids.length]!),
  );
}
export async function reviewProtocol(protocol: ExperimentProtocol, options: RunOptions) {
  options.signal?.throwIfAborted();
  if (!options.onProtocol) return;
  await new Promise<void>((resolve, reject) => {
    // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors -- AbortSignal permits arbitrary reasons; preserve the caller's cancellation identity.
    const abort = () => reject(options.signal?.reason ?? new Error('Отменено'));
    options.signal?.addEventListener('abort', abort, { once: true });
    Promise.resolve()
      .then(() => options.onProtocol!(protocol))
      .then(resolve, reject)
      .finally(() => options.signal?.removeEventListener('abort', abort));
  });
  options.signal?.throwIfAborted();
}

export function createProtocol(
  task: string,
  settings: ModelSettings,
  sourceGrants: readonly [readonly SourceGrant[], readonly SourceGrant[]],
  repetitions: 1 | 3,
  order: readonly (readonly string[])[],
  rubric: EvaluationRubric,
): ExperimentProtocol {
  return freeze({
    version: 3,
    task,
    rubric,
    repetitions,
    participantOrder: order,
    modelSettings: Object.fromEntries(
      componentIds.map((component) => [
        component,
        {
          provider: settings.components[component],
          requestedModel: settings.aliases[component],
          requestedEffort:
            component === 'stalker' || component === 'person-fabric' ? 'medium' : 'high',
        },
      ]),
    ),
    formatAttempts: 2,
    stageTimeoutMs: 600000,
    promptVersion: protocolPromptVersion,
    createdAt: new Date().toISOString(),
    sourceSnapshots: [
      sourceGrants[0].map((grant) => grant.snapshotId),
      sourceGrants[1].map((grant) => grant.snapshotId),
    ],
  });
}
