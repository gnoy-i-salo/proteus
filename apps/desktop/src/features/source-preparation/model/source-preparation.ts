import { createSignal } from 'solid-js';
import type { IngestionBindingRequest, IngestionBudget } from '@proteus-int/contracts';
import { nativeIngestion } from '../../../shared/lib/ingestion';
import { pickIngestionSource, releaseIngestionSource } from '../../../shared/lib/ingestion-input';
import { createSourcePreview } from './source-preview';
import type { SourceFileKind } from './source-preview';
import { createSourceImport } from './source-import';
const importBudget: IngestionBudget = {
  maxInputBytes: 128 * 1024 * 1024,
  maxExpandedBytes: 256 * 1024 * 1024,
  maxDocuments: 100000,
  maxBatchDocuments: 100,
  maxBatchBytes: 1024 * 1024,
  maxDurationMs: 60000,
};
export type SourceImportSelection = Omit<IngestionBindingRequest, 'previewId' | 'personaId'> & {
  readonly allowLocalImport: boolean;
};
export function createSourcePreparation(options: {
  readonly personaId: () => string;
  readonly onCleanupError: (error: unknown) => void;
}) {
  const preview = createSourcePreview({
    port: { ...nativeIngestion, pick: pickIngestionSource, release: releaseIngestionSource },
    budget: () => importBudget,
    onCleanupError: options.onCleanupError,
  });
  const ingestion = createSourceImport({
    port: nativeIngestion,
    onCleanupError: options.onCleanupError,
  });
  const [error, setError] = createSignal('');
  const locked = () => ingestion.busy() || ingestion.job() !== undefined;
  async function choose(kind: SourceFileKind) {
    if (!locked()) {
      setError('');
      await preview.choose(kind);
    }
  }
  async function previewUrl(url: string) {
    if (!locked()) {
      setError('');
      await preview.previewUrl(url);
    }
  }
  async function start(selection: SourceImportSelection) {
    if (locked() || preview.busy()) return;
    setError('');
    const selected = preview.preview();
    const resource = selected?.resources.find((item) => item.id === selection.resourceId);
    if (!selected || !resource || !selection.allowLocalImport) {
      setError('Выберите ресурс и разрешите его локальный импорт.');
      return;
    }
    if (
      (selection.targetAuthorId !== null &&
        !resource.authorIds.includes(selection.targetAuthorId)) ||
      selection.allowedContextAuthorIds.some((id) => !resource.authorIds.includes(id))
    ) {
      setError('Выберите авторов из этого ресурса.');
      return;
    }
    const request = {
      resourceId: selection.resourceId,
      role: selection.role,
      targetAuthorId: selection.targetAuthorId,
      allowedContextAuthorIds: selection.allowedContextAuthorIds,
      allowUnknownContext: selection.allowUnknownContext,
      period: selection.period,
      allowModelProcessing: selection.allowModelProcessing,
      allowExport: selection.allowExport,
    };
    await ingestion.start(
      { ...request, previewId: selected.id, personaId: options.personaId() },
      importBudget,
    );
  }
  const reportError = (cause: unknown) =>
    setError(cause instanceof Error ? cause.message : 'Не удалось подготовить источник.');
  return { preview, ingestion, error, locked, choose, previewUrl, start, reportError };
}
