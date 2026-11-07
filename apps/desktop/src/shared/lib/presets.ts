import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { isDesktop } from './window-controls';
import { downloadFile } from './download-file';

export type PresetFileErrorCode = 'nativeUnavailable' | 'nativeFailure' | 'invalidResponse';
export class PresetFileError extends Error {
  readonly code: PresetFileErrorCode;
  constructor(code: PresetFileErrorCode, message: string) {
    super(message);
    this.name = 'PresetFileError';
    this.code = code;
  }
}

type PendingPreset =
  | { readonly kind: 'ready'; readonly data: string }
  | { readonly kind: 'failed'; readonly error: PresetFileError };

function invalidResponse(): PresetFileError {
  return new PresetFileError('invalidResponse', 'Неверный ответ при открытии файла .proteus.');
}
function presetText(value: unknown): string {
  if (typeof value !== 'string' || new TextEncoder().encode(value).length > 131072)
    throw invalidResponse();
  return value;
}
function nativeFailure(error: unknown, fallback: string): PresetFileError {
  if (error instanceof PresetFileError) return error;
  return new PresetFileError(
    'nativeFailure',
    typeof error === 'string' ? error : error instanceof Error ? error.message : fallback,
  );
}
function pendingPresets(value: unknown): PendingPreset[] {
  if (!Array.isArray(value) || value.length > 16) throw invalidResponse();
  return value.map((file: unknown): PendingPreset => {
    if (!file || typeof file !== 'object' || Array.isArray(file) || Object.keys(file).length !== 1)
      throw invalidResponse();
    // Ok/Err are the exact serde Result representation, isolated at this native boundary.
    const key = Object.keys(file)[0];
    if (key === 'Ok' && 'Ok' in file) return { kind: 'ready', data: presetText(file.Ok) };
    if (key === 'Err' && 'Err' in file)
      return {
        kind: 'failed',
        error: new PresetFileError('nativeFailure', presetText(file.Err)),
      };
    throw invalidResponse();
  });
}

export async function importPreset(): Promise<string | null> {
  if (!isDesktop())
    throw new PresetFileError('nativeUnavailable', 'Выбор файла доступен в настольном приложении.');
  try {
    const result = await invoke<unknown>('import_preset');
    return result === null ? null : presetText(result);
  } catch (error) {
    throw nativeFailure(error, 'Не удалось открыть пресет.');
  }
}
export async function exportPreset(data: string): Promise<void> {
  if (isDesktop()) {
    try {
      const path = await invoke<unknown>('export_preset', { data });
      if (path !== null && typeof path !== 'string') throw invalidResponse();
    } catch (error) {
      throw nativeFailure(error, 'Не удалось сохранить пресет.');
    }
    return;
  }
  downloadFile(data, 'application/x-proteus', 'personas.proteus');
}
export async function listenPresets(
  receive: (data: string) => void,
  fail: (error: PresetFileError) => void,
  signal?: AbortSignal,
): Promise<() => void> {
  if (!isDesktop() || signal?.aborted) return () => {};
  let disposed = false;
  let draining = false;
  let requested = false;
  let unlisten: (() => void) | undefined;
  const dispose = () => {
    if (disposed) return;
    disposed = true;
    signal?.removeEventListener('abort', dispose);
    unlisten?.();
    unlisten = undefined;
  };
  signal?.addEventListener('abort', dispose, { once: true });
  const deliver = (files: ReturnType<typeof pendingPresets>) => {
    for (const file of files) {
      if (disposed) break;
      if (file.kind === 'ready') receive(file.data);
      else fail(file.error);
    }
  };
  const drain = async () => {
    requested = true;
    if (draining || disposed) return;
    draining = true;
    try {
      while (requested && !disposed) {
        requested = false;
        try {
          const raw = await invoke<unknown>('take_pending_presets');
          if (disposed) break;
          deliver(pendingPresets(raw));
        } catch (error) {
          if (!disposed) fail(nativeFailure(error, 'Не удалось прочитать очередь пресетов.'));
        }
      }
    } finally {
      draining = false;
    }
  };
  try {
    const stop = await listen('preset-opened', () => {
      void drain().catch((error: unknown) => {
        // A broken owner error callback cannot receive another error. Release its
        // listener and report through the browser's uncaught-error channel.
        dispose();
        reportOwnerFailure(error);
      });
    });
    if (disposed) stop();
    else {
      unlisten = stop;
      await drain();
    }
    return dispose;
  } catch (error) {
    dispose();
    throw nativeFailure(error, 'Не удалось включить открытие пресетов.');
  }
}

function reportOwnerFailure(error: unknown): void {
  if (typeof globalThis.reportError === 'function') {
    globalThis.reportError(error);
    return;
  }
  // Older WebKit still supports ErrorEvent. Preserve the original thrown value
  // for the owner's uncaught-error listener without starting another Promise.
  setTimeout(() => {
    globalThis.dispatchEvent(
      new ErrorEvent('error', {
        error,
        message: error instanceof Error ? error.message : 'Unhandled preset callback failure',
      }),
    );
  }, 0);
}
