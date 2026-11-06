import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { isDesktop } from './window-controls';
export async function importPreset(): Promise<string | null> {
  return invoke('import_preset');
}
export async function exportPreset(data: string): Promise<void> {
  if (isDesktop()) {
    await invoke('export_preset', { data });
    return;
  }
  const url = URL.createObjectURL(new Blob([data], { type: 'application/x-proteus' }));
  const link = document.createElement('a');
  link.href = url;
  link.download = 'personas.proteus';
  link.click();
  URL.revokeObjectURL(url);
}
export async function listenPresets(
  receive: (data: string) => void,
  fail: (error: string) => void,
): Promise<() => void> {
  if (!isDesktop()) return () => {};
  let disposed = false;
  let draining = false;
  let requested = false;
  const drain = async () => {
    requested = true;
    if (draining || disposed) return;
    draining = true;
    try {
      while (requested && !disposed) {
        requested = false;
        const files = await invoke<({ Ok: string } | { Err: string })[]>('take_pending_presets');
        if (disposed) break;
        for (const file of files) {
          if ('Ok' in file) receive(file.Ok);
          else fail(file.Err);
        }
      }
    } catch (error) {
      if (!disposed) fail(String(error));
    } finally {
      draining = false;
    }
  };
  const unlisten = await listen('preset-opened', () => {
    void drain();
  });
  await drain();
  return () => {
    disposed = true;
    unlisten();
  };
}
