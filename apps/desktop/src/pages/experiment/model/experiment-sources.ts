import { createSignal } from 'solid-js';
import type { ImportedSource, SourceGrant, SourceProvider } from '@proteus-int/contracts';
import { deleteSourceGrant } from '../../../shared/lib/connections';

export function createExperimentSources(
  removeGrant: typeof deleteSourceGrant = deleteSourceGrant,
  reportError: (message: string) => void,
) {
  const [sourceGrants, setSourceGrants] = createSignal<
    readonly [readonly SourceGrant[], readonly SourceGrant[]]
  >([[], []]);
  const [sourceImports, setSourceImports] = createSignal<
    readonly { provider: SourceProvider; source: ImportedSource }[]
  >([]);
  const rememberImport = (provider: SourceProvider, source: ImportedSource) =>
    setSourceImports((items) => [
      ...items.filter((item) => item.source.id !== source.id),
      { provider, source },
    ]);
  const clearSourceGrants = () => {
    for (const grant of sourceGrants().flat()) {
      void removeGrant(grant.id).catch(() =>
        reportError(
          'Источники убраны из эксперимента, но не удалось подтвердить отзыв разрешений. Удалите материалы в настройках подключений и повторите попытку.',
        ),
      );
    }
    setSourceGrants([[], []]);
  };
  const clearProvider = (provider: SourceProvider) => {
    setSourceImports((items) => items.filter((item) => item.provider !== provider));
    setSourceGrants((grants) => [
      grants[0].filter((grant) => grant.provider !== provider),
      grants[1].filter((grant) => grant.provider !== provider),
    ]);
  };
  const changeGrants = (person: number, grants: readonly SourceGrant[]) =>
    setSourceGrants((current) => (person === 0 ? [grants, current[1]] : [current[0], grants]));
  return {
    sourceGrants,
    sourceImports,
    rememberImport,
    clearSourceGrants,
    clearProvider,
    changeGrants,
  };
}
