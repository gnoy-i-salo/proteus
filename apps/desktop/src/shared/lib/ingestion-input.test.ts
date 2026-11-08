import { beforeEach, expect, it, vi } from 'vitest';
import { pickIngestionSource, releaseIngestionSource } from './ingestion-input';

const native = vi.hoisted(() => ({ desktop: true, invoke: vi.fn() }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: native.invoke }));
vi.mock('./window-controls', () => ({ isDesktop: () => native.desktop }));
const selected = {
  schemaVersion: 1,
  sourceHandle: 'A'.repeat(32),
  label: '  Архив 👩🏽‍💻.json',
  inputBytes: 20_000_000,
};
beforeEach(() => {
  vi.resetAllMocks();
  native.desktop = true;
});

it('receives a native-selected capability without sending a filesystem path', async () => {
  native.invoke.mockResolvedValue(selected);
  expect(await pickIngestionSource()).toEqual(selected);
  expect(native.invoke).toHaveBeenCalledExactlyOnceWith('pick_ingestion_source');
});
it('returns null when the user cancels the native picker', async () => {
  native.invoke.mockResolvedValue(null);
  expect(await pickIngestionSource()).toBeNull();
});
it('explicitly releases a selected capability', async () => {
  native.invoke.mockResolvedValue(undefined);
  await releaseIngestionSource(selected.sourceHandle);
  expect(native.invoke).toHaveBeenCalledExactlyOnceWith('release_ingestion_source', {
    sourceHandle: selected.sourceHandle,
  });
});
it('does not invoke native operations in the browser preview', async () => {
  native.desktop = false;
  await expect(pickIngestionSource()).rejects.toThrow();
  await expect(releaseIngestionSource(selected.sourceHandle)).rejects.toThrow();
  expect(native.invoke).not.toHaveBeenCalled();
});
it.each([
  undefined,
  { ...selected, schemaVersion: 2 },
  { ...selected, path: '/private/file' },
  { ...selected, inputBytes: -1 },
  { ...selected, inputBytes: 1.5 },
  { ...selected, inputBytes: Number.MAX_SAFE_INTEGER + 1 },
  { ...selected, sourceHandle: '/private/file' },
  { ...selected, label: 3 },
])('rejects unsupported native selection responses (%#)', async (value) => {
  native.invoke.mockResolvedValue(value);
  await expect(pickIngestionSource()).rejects.toThrow();
});
it.each(['', '/private/file', 'A'.repeat(31), 'A'.repeat(33), ' '.repeat(32)])(
  'rejects malformed release handles before IPC (%#)',
  async (handle) => {
    await expect(releaseIngestionSource(handle)).rejects.toThrow();
    expect(native.invoke).not.toHaveBeenCalled();
  },
);

it('releases an opaque native URL capability without accepting an actual URL', async () => {
  const handle = `url:${selected.sourceHandle}`;
  native.invoke.mockResolvedValue(undefined);
  await releaseIngestionSource(handle);
  expect(native.invoke).toHaveBeenCalledExactlyOnceWith('release_ingestion_source', {
    sourceHandle: handle,
  });
  native.invoke.mockClear();
  await expect(releaseIngestionSource('url:https://example.com/')).rejects.toThrow();
  expect(native.invoke).not.toHaveBeenCalled();
});
