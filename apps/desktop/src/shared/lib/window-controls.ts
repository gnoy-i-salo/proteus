import { isTauri } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
export function isDesktop(): boolean {
  return isTauri();
}
export async function controlWindow(action: 'minimize' | 'maximize' | 'close'): Promise<void> {
  if (!isDesktop()) return;
  const window = getCurrentWindow();
  if (action === 'minimize') await window.minimize();
  else if (action === 'maximize') await window.toggleMaximize();
  else await window.close();
}
