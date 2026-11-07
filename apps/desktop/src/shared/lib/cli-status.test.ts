import { beforeEach, expect, it, vi } from 'vitest';
import { cliStatus, saveExperiment } from './cli';
const native = vi.hoisted(() => ({ desktop: true, invoke: vi.fn() }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: native.invoke }));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn() }));
vi.mock('./window-controls', () => ({ isDesktop: () => native.desktop }));
beforeEach(() => {
  vi.clearAllMocks();
  native.desktop = true;
});

it('preserves native availability and literal platform paths', async () => {
  const status = [
    { provider: 'codex', available: true, path: String.raw`C:\Program Files\Путь\codex.exe` },
    { provider: 'kimi', available: false, path: null },
  ];
  native.invoke.mockResolvedValue(status);
  expect(await cliStatus()).toEqual(status);
  expect(native.invoke).toHaveBeenCalledWith('cli_status');
});
it.each([
  undefined,
  null,
  {},
  [{ provider: 'invented', available: false, path: null }],
  [{ provider: 'codex', available: true }],
  [{ provider: 'codex', available: true, path: null }],
  [{ provider: 'codex', available: false, path: '/usr/bin/codex' }],
  [{ provider: 'codex', available: 'yes', path: '/usr/bin/codex' }],
  [{ provider: 'codex', available: true, path: 3 }],
  [{ provider: 'codex', available: true, path: '/usr/bin/codex', token: 'synthetic-only' }],
  [
    { provider: 'codex', available: false, path: null },
    { provider: 'codex', available: false, path: null },
  ],
])('rejects malformed or inconsistent native availability (%#)', async (value) => {
  native.invoke.mockResolvedValue(value);
  await expect(cliStatus()).rejects.toThrow();
});
it.each([
  '/tmp/Точный путь/result.json',
  String.raw`C:\Users\Name\PROTEUS\experiments\result.json`,
  '  exact path  ',
])('preserves the native save path without normalization (%#)', async (path) => {
  native.invoke.mockResolvedValue(path);
  const data = { arbitrary: { 'source-url': ' exact\r\n' } };
  expect(await saveExperiment(data)).toBe(path);
  expect(native.invoke).toHaveBeenCalledWith('save_experiment', {
    id: expect.any(String),
    data: JSON.stringify(data),
  });
});
it.each([null, undefined, 3, { path: '/fake' }])(
  'rejects invalid save responses (%#)',
  async (value) => {
    native.invoke.mockResolvedValue(value);
    await expect(saveExperiment({})).rejects.toThrow();
  },
);
it('does not invoke native availability or save commands in browser mode', async () => {
  native.desktop = false;
  expect(await cliStatus()).toEqual([]);
  expect(await saveExperiment({})).toBe('');
  expect(native.invoke).not.toHaveBeenCalled();
});
