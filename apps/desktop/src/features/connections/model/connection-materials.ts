import { createSignal } from 'solid-js';
import type { ImportedSource, SourceProvider } from '@proteus-int/contracts';
import { clearSourceData, importSource } from '../../../shared/lib/connections';
import type { ConnectionActivity } from './connection-session';

export interface ConnectionMaterialsPort {
  readonly importSource: typeof importSource;
  readonly clearSourceData: typeof clearSourceData;
}
export function createConnectionMaterials(
  selected: () => SourceProvider,
  activity: ConnectionActivity,
  callbacks: {
    readonly onImported: (provider: SourceProvider, source: ImportedSource) => void;
    readonly onDataCleared: (provider: SourceProvider) => void;
  },
  port: ConnectionMaterialsPort = { importSource, clearSourceData },
) {
  const [imported, setImported] = createSignal<ImportedSource>();
  const [clearConfirm, setClearConfirm] = createSignal(false);
  const [cleared, setCleared] = createSignal(false);
  const pickImport = async () => {
    if (!activity.start()) return;
    setCleared(false);
    const provider = selected();
    try {
      const source = await port.importSource(provider);
      if (activity.active() && source) {
        setImported(source);
        callbacks.onImported(provider, source);
      }
    } catch {
      if (activity.active())
        activity.fail(
          'Не удалось импортировать файл. Выберите поддерживаемый JSON экспорт или текст.',
        );
    } finally {
      if (activity.active()) activity.finish();
    }
  };
  const clearData = async () => {
    if (activity.busy()) return;
    if (!clearConfirm()) {
      setClearConfirm(true);
      return;
    }
    if (!activity.start()) return;
    const provider = selected();
    try {
      await port.clearSourceData(provider);
      if (activity.active()) {
        setImported(undefined);
        setCleared(true);
        setClearConfirm(false);
        callbacks.onDataCleared(provider);
      }
    } catch {
      if (activity.active())
        activity.fail('Не удалось удалить загруженные данные. Повторите попытку.');
    } finally {
      if (activity.active()) activity.finish();
    }
  };

  const reset = () => {
    setImported(undefined);
    setClearConfirm(false);
    setCleared(false);
  };
  return { imported, clearConfirm, cleared, pickImport, clearData, reset };
}
