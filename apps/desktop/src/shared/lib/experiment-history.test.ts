import { beforeEach, expect, it, vi } from 'vitest';
import { listSavedExperiments, readSavedExperiment } from './experiment-history';
const native = vi.hoisted(() => ({
  desktop: true,
  invoke: vi.fn<(command: string, args?: Record<string, unknown>) => Promise<unknown>>(),
}));
vi.mock('@tauri-apps/api/core', () => ({ invoke: native.invoke }));
vi.mock('./window-controls', () => ({ isDesktop: () => native.desktop }));
const page = {
  schemaVersion: 1,
  entries: [
    { id: 'run-1', ok: true, error: null },
    { id: 'run-2', ok: false, error: 'missingResult' },
  ],
  matched: 3,
  nextOffset: 2,
  scanTruncated: false,
};
const saved = {
  schemaVersion: 1,
  id: 'run-1',
  text: '{\r\n  "task": "  Анна → Борис é 😀\\r\\n  "\r\n}\r\n',
};
beforeEach(() => {
  vi.resetAllMocks();
  native.desktop = true;
});
it('lists bounded native history without loading every result or starting a model', async () => {
  native.invoke.mockResolvedValueOnce(page);
  expect(await listSavedExperiments(0, 2)).toEqual(page);
  expect(native.invoke).toHaveBeenCalledExactlyOnceWith('list_saved_experiments', {
    offset: 0,
    limit: 2,
  });
});
it('reads an opaque native ID and preserves exact result bytes without rewriting or upgrading', async () => {
  native.invoke.mockResolvedValueOnce(saved);
  expect(await readSavedExperiment('run-1')).toEqual(saved);
  expect(native.invoke).toHaveBeenCalledExactlyOnceWith('read_saved_experiment', { id: 'run-1' });
});
it('rejects path injection, invalid bounds, web and pre-cancelled requests before IPC', async () => {
  for (const id of ['', '../run-1', 'a/b', 'a\\b', 'run_1', 'x'.repeat(81)])
    await expect(readSavedExperiment(id)).rejects.toThrow();
  for (const [offset, limit] of [
    [-1, 10],
    [1.5, 10],
    [1025, 10],
    [0, 0],
    [0, 101],
    [0, NaN],
  ])
    await expect(listSavedExperiments(offset, limit)).rejects.toThrow();
  native.desktop = false;
  await expect(listSavedExperiments()).rejects.toThrow();
  await expect(readSavedExperiment('run-1')).rejects.toThrow();
  native.desktop = true;
  const cancel = new AbortController();
  cancel.abort();
  await expect(readSavedExperiment('run-1', cancel.signal)).rejects.toThrow();
  await expect(listSavedExperiments(0, 50, cancel.signal)).rejects.toThrow();
  expect(native.invoke).not.toHaveBeenCalled();
});
it('rejects mixed fields, invalid bounds and inconsistent pagination without hiding damaged entries', async () => {
  for (const bad of [
    { ...page, schemaVersion: 2 },
    { ...page, extra: true },
    { ...page, matched: 1025 },
    { ...page, nextOffset: 1 },
    { ...page, matched: 1 },
    { ...page, scanTruncated: 'yes' },
    { ...page, entries: [page.entries[0], page.entries[0]] },
    { ...page, entries: [{ id: 'run-1', ok: true, error: 'missingResult' }, page.entries[1]] },
    { ...page, entries: [{ id: 'run-1', ok: false, error: null }, page.entries[1]] },
    { ...page, entries: [{ id: 'run-1', ok: false, error: 'unknown' }, page.entries[1]] },
  ]) {
    native.invoke.mockResolvedValueOnce(bad);
    await expect(listSavedExperiments(0, 2)).rejects.toThrow();
  }
});
it('rejects corrupt, oversized and foreign result envelopes while retaining old JSON object schemas', async () => {
  for (const bad of [
    { ...saved, id: 'foreign' },
    { ...saved, schemaVersion: 2 },
    { ...saved, extra: true },
    { ...saved, text: 'null' },
    { ...saved, text: '[]' },
    { ...saved, text: '{' },
    { ...saved, text: '\ud800' },
    { ...saved, text: 'x'.repeat(8388609) },
  ]) {
    native.invoke.mockResolvedValueOnce(bad);
    await expect(readSavedExperiment('run-1')).rejects.toThrow();
  }
  native.invoke.mockResolvedValueOnce({
    ...saved,
    text: '{"finished-at":"2025-01-01","people":[]}',
  });
  expect((await readSavedExperiment('run-1')).text).toContain('finished-at');
});
it('discards a late read after cancellation and preserves explicit native failures', async () => {
  const cancel = new AbortController();
  native.invoke.mockImplementationOnce(() => {
    cancel.abort(new Error('stale history'));
    return Promise.resolve(saved);
  });
  await expect(readSavedExperiment('run-1', cancel.signal)).rejects.toThrow('stale history');
  native.invoke.mockRejectedValueOnce(new Error('disk unavailable'));
  await expect(listSavedExperiments()).rejects.toThrow('disk unavailable');
});

it('retains explicit partial scan coverage and accepts the exact native byte limit', async () => {
  native.invoke.mockResolvedValueOnce({ ...page, scanTruncated: true });
  expect((await listSavedExperiments(0, 2)).scanTruncated).toBe(true);
  const text = '{"text":"' + 'x'.repeat(8 * 1024 * 1024 - 11) + '"}';
  expect(new TextEncoder().encode(text)).toHaveLength(8 * 1024 * 1024);
  native.invoke.mockResolvedValueOnce({ ...saved, text });
  expect((await readSavedExperiment(saved.id)).text).toBe(text);
});
