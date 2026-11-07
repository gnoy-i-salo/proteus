import { createSignal, onCleanup, untrack } from 'solid-js';
import type { ImportedSource, SourceGrant, SourceProvider } from '@proteus-int/contracts';
import {
  deleteSourceGrant,
  createSourceGrant,
  importSource,
} from '../../../shared/lib/connections';

export interface SourceSelectionFields {
  readonly author: string;
  readonly from: string;
  readonly to: string;
}
export interface SourceSelectionPort {
  readonly importSource: typeof importSource;
  readonly createSourceGrant: typeof createSourceGrant;
  readonly deleteSourceGrant: typeof deleteSourceGrant;
}
export function createSourceSelectionModel(
  options: {
    readonly grants: () => readonly SourceGrant[];
    readonly modelProcessingAvailable: () => boolean;
    readonly onImported: (provider: SourceProvider, source: ImportedSource) => void;
    readonly onChange: (grants: readonly SourceGrant[]) => void;
  },
  port: SourceSelectionPort = { importSource, createSourceGrant, deleteSourceGrant },
) {
  const [provider, setProvider] = createSignal<SourceProvider>('telegram');
  const [resource, setResource] = createSignal('');
  const [consent, setConsent] = createSignal(false);
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal('');
  const [selectedGrant, setSelectedGrant] = createSignal(0);
  const [adding, setAdding] = createSignal(untrack(() => options.grants().length === 0));
  let disposed = false;
  const current = () => options.grants()[Math.min(selectedGrant(), options.grants().length - 1)];
  onCleanup(() => {
    disposed = true;
  });
  const pick = async () => {
    setBusy(true);
    setError('');
    try {
      const source = await port.importSource(provider());
      if (source && !disposed) {
        options.onImported(provider(), source);
        setResource(source.id);
        setConsent(false);
      }
    } catch {
      if (!disposed)
        setError('Не удалось прочитать файл. Выберите поддерживаемый JSON или текстовый экспорт.');
    } finally {
      if (!disposed) setBusy(false);
    }
  };
  const add = async (fields: SourceSelectionFields) => {
    if (!consent() || busy() || options.grants().length >= 5 || !options.modelProcessingAvailable())
      return;
    const date = (key: 'from' | 'to', end: boolean) => {
      const value = fields[key];
      return value ? `${value}T${end ? '23:59:59.999' : '00:00:00.000'}Z` : null;
    };
    setBusy(true);
    setError('');
    try {
      const grant = await port.createSourceGrant({
        provider: provider(),
        resourceId: resource().trim(),
        authorId: fields.author.trim() || null,
        from: date('from', false),
        to: date('to', true),
        allowModelProcessing: true,
        operations: ['describe', 'search', 'collect', 'read', 'coverage'],
        budget: {
          maxSteps: 40,
          maxDocuments: 1000,
          maxBytes: 1048576,
          maxDurationMs: 300000,
        },
      });
      if (!disposed) {
        options.onChange([...options.grants(), grant]);
        setSelectedGrant(options.grants().length - 1);
        setAdding(false);
        setConsent(false);
      }
    } catch {
      if (!disposed)
        setError(
          'Не удалось разрешить доступ. Проверьте подключение, ресурс, автора и диапазон дат.',
        );
    } finally {
      if (!disposed) setBusy(false);
    }
  };
  const remove = async () => {
    const grant = current();
    if (!grant) return;
    setBusy(true);
    setError('');
    try {
      await port.deleteSourceGrant(grant.id);
      if (!disposed) {
        options.onChange(options.grants().filter((item) => item.id !== grant.id));
        if (!options.grants().length) setAdding(true);
      }
    } catch {
      if (!disposed) setError('Не удалось отменить доступ. Повторите попытку.');
    } finally {
      if (!disposed) setBusy(false);
    }
  };
  const selectProvider = (value: SourceProvider) => {
    setProvider(value);
    setResource('');
    setConsent(false);
    setError('');
  };
  const changeResource = (value: string) => {
    setResource(value);
    setConsent(false);
  };
  return {
    provider,
    resource,
    consent,
    busy,
    error,
    selectedGrant,
    adding,
    current,
    setConsent,
    setSelectedGrant,
    setAdding,
    selectProvider,
    changeResource,
    pick,
    add,
    remove,
    reportActionFailure: () => {
      if (!disposed) setError('Не удалось завершить действие с источником. Повторите попытку.');
    },
  };
}
