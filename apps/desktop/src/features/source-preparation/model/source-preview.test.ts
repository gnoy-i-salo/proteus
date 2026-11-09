import type * as SolidClient from 'solid-js';
import { createRoot } from 'solid-js';
import { expect, it, vi } from 'vitest';
import type { IngestionBudget, IngestionPreview } from '@proteus-int/contracts';
import { createSourcePreview } from './source-preview';
import type { SourcePreviewPort } from './source-preview';
vi.mock('solid-js', () => vi.importActual<typeof SolidClient>('solid-js/dist/solid.js'));
const budget: IngestionBudget = {
  maxInputBytes: 1000000,
  maxExpandedBytes: 2000000,
  maxDocuments: 1000,
  maxBatchDocuments: 100,
  maxBatchBytes: 100000,
  maxDurationMs: 30000,
};
const input = {
  schemaVersion: 1 as const,
  sourceHandle: 'a'.repeat(32),
  label: '  Архив Ёлка.json',
  inputBytes: 8192,
};
const preview: IngestionPreview = {
  schemaVersion: 1,
  id: 'preview',
  sourceHandle: input.sourceHandle,
  sourceFormat: 'telegramDesktopJson',
  adapterVersion: 'telegram:1',
  resources: [{ id: 'chat', label: 'Точный чат', authorIds: ['author'], estimatedDocuments: 100 }],
  limitations: ['Only metadata, no model study'],
};
function setup() {
  const port = {
    pick: vi.fn<SourcePreviewPort['pick']>().mockResolvedValue(input),
    release: vi.fn<SourcePreviewPort['release']>().mockResolvedValue(undefined),
    preview: vi.fn<SourcePreviewPort['preview']>().mockResolvedValue(preview),
    previewSavedPage: vi
      .fn<SourcePreviewPort['previewSavedPage']>()
      .mockResolvedValue({ ...preview, sourceFormat: 'savedPageHtml' }),
    previewUrl: vi.fn<SourcePreviewPort['previewUrl']>().mockResolvedValue({
      ...preview,
      sourceHandle: 'url:' + 'b'.repeat(32),
      sourceFormat: 'selectedUrlHtml',
    }),
  };
  const onCleanupError = vi.fn();
  let dispose!: () => void;
  const model = createRoot((stop) => {
    dispose = stop;
    return createSourcePreview({ port, budget: () => budget, onCleanupError });
  });
  return { port, model, dispose, onCleanupError };
}
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}
it('selects a native archive and exposes metadata without importing or creating permissions', async () => {
  const { model, port, dispose } = setup();
  await model.choose('telegramArchive');
  expect(model.selected()).toEqual(input);
  expect(model.preview()).toEqual(preview);
  expect(port.preview).toHaveBeenCalledWith(input.sourceHandle, budget, expect.any(AbortSignal));
  expect(model.error()).toBe('');
  expect(model.busy()).toBe(false);
  dispose();
  await Promise.resolve();
  expect(port.release).toHaveBeenCalledExactlyOnceWith(input.sourceHandle);
});
it('releases a file selected after the screen was disposed and never previews it', async () => {
  const { model, port, dispose } = setup();
  const picked = deferred<typeof input | null>();
  port.pick.mockReturnValueOnce(picked.promise);
  const pending = model.choose('telegramArchive');
  dispose();
  picked.resolve(input);
  await pending;
  expect(port.release).toHaveBeenCalledExactlyOnceWith(input.sourceHandle);
  expect(port.preview).not.toHaveBeenCalled();
  expect(model.preview()).toBeUndefined();
});
it('cancels an old preview before a new selection and ignores its late metadata', async () => {
  const { model, port, dispose } = setup();
  const old = deferred<IngestionPreview>();
  port.preview.mockReturnValueOnce(old.promise);
  const pending = model.choose('telegramArchive');
  await Promise.resolve();
  await Promise.resolve();
  const priorSignal = port.preview.mock.calls[0]![2]!;
  model.cancel();
  const nextInput = { ...input, sourceHandle: 'c'.repeat(32) };
  port.pick.mockResolvedValueOnce(nextInput);
  port.preview.mockResolvedValueOnce({
    ...preview,
    id: 'new',
    sourceHandle: nextInput.sourceHandle,
  });
  await model.choose('telegramArchive');
  old.resolve(preview);
  await pending;
  expect(priorSignal.aborted).toBe(true);
  expect(model.preview()?.id).toBe('new');
  expect(port.release).toHaveBeenCalledWith(input.sourceHandle);
  dispose();
});

it.each([
  ['savedHtml', 'text/html'],
  ['savedText', 'text/plain'],
  ['savedMarkdown', 'text/markdown'],
] as const)('uses the explicit saved-page format %s', async (kind, contentType) => {
  const { model, port, dispose } = setup();
  await model.choose(kind);
  expect(port.previewSavedPage).toHaveBeenCalledWith(
    input.sourceHandle,
    contentType,
    budget,
    expect.any(AbortSignal),
  );
  expect(port.preview).not.toHaveBeenCalled();
  dispose();
});
it('keeps the prior preview when the native file picker is dismissed', async () => {
  const { model, port, dispose } = setup();
  await model.choose('telegramArchive');
  port.pick.mockResolvedValueOnce(null);
  await model.choose('telegramArchive');
  expect(model.preview()).toEqual(preview);
  expect(port.release).not.toHaveBeenCalled();
  dispose();
});
it('does not leave old source metadata selectable while a new URL is being read', async () => {
  const { model, port, dispose } = setup();
  await model.choose('telegramArchive');
  const response = deferred<IngestionPreview>();
  port.previewUrl.mockReturnValueOnce(response.promise);
  const pending = model.previewUrl('https://example.org/Exact-ID');
  expect(model.busy()).toBe(true);
  expect(model.preview()).toBeUndefined();
  expect(model.selected()).toBeUndefined();
  response.resolve({
    ...preview,
    sourceHandle: 'url:' + 'b'.repeat(32),
    sourceFormat: 'selectedUrlHtml',
  });
  await pending;
  expect(port.previewUrl).toHaveBeenCalledWith(
    'https://example.org/Exact-ID',
    budget,
    expect.any(AbortSignal),
  );
  expect(model.preview()?.sourceFormat).toBe('selectedUrlHtml');
  expect(model.selected()).toBeUndefined();
  dispose();
});
it('releases a URL capability returned after cancellation without showing its metadata', async () => {
  const { model, port, dispose } = setup();
  const response = deferred<IngestionPreview>();
  port.previewUrl.mockReturnValueOnce(response.promise);
  const pending = model.previewUrl('https://example.org/Exact-ID');
  model.cancel();
  const sourceHandle = 'url:' + 'b'.repeat(32);
  response.resolve({ ...preview, sourceHandle });
  await pending;
  expect(model.preview()).toBeUndefined();
  expect(port.release).toHaveBeenCalledExactlyOnceWith(sourceHandle);
  dispose();
});
it('reports native denial without inventing a successful preview', async () => {
  const { model, port, dispose } = setup();
  port.preview.mockRejectedValueOnce(new Error('Access revoked'));
  await model.choose('telegramArchive');
  expect(model.error()).toBe('Access revoked');
  expect(model.preview()).toBeUndefined();
  expect(model.busy()).toBe(false);
  dispose();
});
it('reports capability cleanup failure to the screen owner', async () => {
  const { model, port, dispose, onCleanupError } = setup();
  await model.choose('telegramArchive');
  const failure = new Error('Release failed');
  port.release.mockRejectedValueOnce(failure);
  dispose();
  await vi.waitFor(() => expect(onCleanupError).toHaveBeenCalledExactlyOnceWith(failure));
});
