import { isTauri } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';

export type WindowAction = 'minimize' | 'maximize' | 'fullscreen' | 'close';
export type WindowPlatform = 'macos' | 'other';

export function isDesktop(): boolean {
  return isTauri();
}

export function windowPlatform(platform = globalThis.navigator?.platform ?? ''): WindowPlatform {
  return platform.startsWith('Mac') ? 'macos' : 'other';
}

export function windowActions(platform: WindowPlatform): readonly WindowAction[] {
  return platform === 'macos'
    ? ['close', 'minimize', 'fullscreen']
    : ['minimize', 'maximize', 'close'];
}

export async function controlWindow(action: WindowAction): Promise<void> {
  if (!isDesktop()) return;
  const window = getCurrentWindow();
  if (action === 'minimize') await window.minimize();
  else if (action === 'maximize') await window.toggleMaximize();
  else if (action === 'fullscreen') await window.setFullscreen(!(await window.isFullscreen()));
  else await window.close();
}
