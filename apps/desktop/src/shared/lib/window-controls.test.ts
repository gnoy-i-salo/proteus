import { beforeEach, expect, it, vi } from 'vitest';
import { controlWindow, windowActions, windowPlatform } from './window-controls';

const native = vi.hoisted(() => ({
  isTauri: vi.fn(),
  getCurrentWindow: vi.fn(),
  minimize: vi.fn(),
  toggleMaximize: vi.fn(),
  close: vi.fn(),
  isFullscreen: vi.fn(),
  setFullscreen: vi.fn(),
}));
vi.mock('@tauri-apps/api/core', () => ({ isTauri: native.isTauri }));
vi.mock('@tauri-apps/api/window', () => ({ getCurrentWindow: native.getCurrentWindow }));

beforeEach(() => {
  vi.clearAllMocks();
  native.isTauri.mockReturnValue(true);
  native.getCurrentWindow.mockReturnValue(native);
});

it('orders close, minimize and fullscreen on macOS; maximize stays on other platforms', () => {
  expect(windowPlatform('MacIntel')).toBe('macos');
  expect(windowPlatform('Win32')).toBe('other');
  expect(windowPlatform('Linux x86_64')).toBe('other');
  expect(windowActions('macos')).toEqual(['close', 'minimize', 'fullscreen']);
  expect(windowActions('other')).toEqual(['minimize', 'maximize', 'close']);
});

it('toggles the actual native fullscreen state in both directions', async () => {
  native.isFullscreen.mockResolvedValueOnce(false).mockResolvedValueOnce(true);
  await controlWindow('fullscreen');
  await controlWindow('fullscreen');
  expect(native.setFullscreen.mock.calls).toEqual([[true], [false]]);
  expect(native.toggleMaximize).not.toHaveBeenCalled();
  expect(native.close).not.toHaveBeenCalled();
});

it('browser preview never invokes native window commands', async () => {
  native.isTauri.mockReturnValue(false);
  for (const action of ['close', 'minimize', 'maximize', 'fullscreen'] as const)
    await controlWindow(action);
  expect(native.getCurrentWindow).not.toHaveBeenCalled();
});

it('keeps minimize/maximize/close separate and exposes native failures', async () => {
  await controlWindow('minimize');
  await controlWindow('maximize');
  await controlWindow('close');
  expect(native.minimize).toHaveBeenCalledOnce();
  expect(native.toggleMaximize).toHaveBeenCalledOnce();
  expect(native.close).toHaveBeenCalledOnce();
  native.setFullscreen.mockRejectedValueOnce(new Error('Window unavailable'));
  await expect(controlWindow('fullscreen')).rejects.toThrow('Window unavailable');
});
