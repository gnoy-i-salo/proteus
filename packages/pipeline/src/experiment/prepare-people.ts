import type {
  ExperimentInput,
  PersonProfile,
  SourceGrant,
  SourcePort,
} from '@proteus-int/contracts';
import { analyzeSourceCollections, collectPerson, collectSource } from '@proteus-int/stalker';
import type { SourceCollection, StalkerReport } from '@proteus-int/stalker';
import { adaptPerson, fabricatePerson, neutralProfile } from '@proteus-int/person-fabric';
import type { StageRunner } from '../execution/run-stage';
import { freeze } from './input-snapshot';
import type { RunOptions } from './run-options';

export interface PreparedPerson {
  readonly report: StalkerReport | null;
  readonly base: PersonProfile;
  profile: PersonProfile;
}

export async function preparePeople(
  input: ExperimentInput,
  sourceGrants: readonly [readonly SourceGrant[], readonly SourceGrant[]],
  sourcePort: SourcePort | undefined,
  stage: StageRunner,
  options: Pick<RunOptions, 'signal'>,
): Promise<PreparedPerson[]> {
  const task = input.task;
  const prepared: PreparedPerson[] = [];
  for (const [index, person] of input.people.entries()) {
    const { report } = await stage(
      `sources-${index}`,
      'stalker',
      index,
      'Наблюдения и источники',
      async (provider) => {
        const grants = sourceGrants[index]!;
        if (!grants.length) return collectPerson(person, provider, options.signal);
        const collections: SourceCollection[] = [];
        for (const grant of grants) {
          const collection = await collectSource(
            grant,
            person,
            provider,
            sourcePort!,
            options.signal,
          );
          if (collection.status === 'cancelled') {
            options.signal?.throwIfAborted();
            throw new Error('Сбор выбранного источника отменён.');
          }
          collections.push(collection);
        }
        return analyzeSourceCollections(person, collections, provider, sourcePort!, options.signal);
      },
    );
    const { profile } = await stage(
      `profile-${index}`,
      'person-fabric',
      index,
      'Устойчивый профиль',
      (provider) =>
        fabricatePerson(
          report,
          person,
          index === 0 ? 'a' : 'b',
          index === 0 ? 'turquoise' : 'violet',
          provider,
          options.signal,
        ),
    );
    prepared.push({ report, base: freeze(profile), profile });
  }
  const neutral = freeze(neutralProfile());
  prepared.push({ report: null, base: neutral, profile: neutral });
  for (const [index, person] of prepared.entries()) {
    const { profile } = await stage(
      `adapt-${index}`,
      'person-fabric',
      index,
      'Стратегия для задачи',
      (provider) => adaptPerson(person.base, task, provider, options.signal),
    );
    person.profile = freeze(profile);
  }
  return prepared;
}
