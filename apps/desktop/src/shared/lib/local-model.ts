import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import {
  decodeLocalModelPackageState,
  decodeLocalModelProgress,
  decodeLocalModelState,
} from './local-model-wire';
import type { LocalModelProgress } from './local-model-wire';
import { isDesktop } from './window-controls';

function available(signal?: AbortSignal): void {
  if (!isDesktop()) throw new Error('Локальная модель доступна в настольном приложении PROTEUS.');
  signal?.throwIfAborted();
}

// Native admission happens inside spawn_blocking. Retry a cancellation that
// arrives before admission, with at most one request in flight. Await the worker
// itself before reporting completion: an acknowledgment is not a stopped worker.
async function operation<T>(
  command: string,
  operationId: string,
  decode: (value: unknown) => T,
  signal?: AbortSignal,
): Promise<T> {
  available(signal);
  let pending = true;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let cancellation: Promise<void> | undefined;
  const cancel = () => {
    if (!pending) return;
    cancellation = (async () => {
      let acknowledged = false;
      try {
        acknowledged =
          (await invoke<unknown>('cancel_local_model_operation', { operationId })) === true;
      } catch {
        // Retry transport failure too; a completed worker ends retries below.
      }
      if (pending && !acknowledged) timer = setTimeout(cancel, 100);
    })();
  };
  signal?.addEventListener('abort', cancel, { once: true });
  try {
    const value = await invoke<unknown>(command, { operationId });
    signal?.throwIfAborted();
    return decode(value);
  } catch (error) {
    signal?.throwIfAborted();
    throw error;
  } finally {
    pending = false;
    clearTimeout(timer);
    signal?.removeEventListener('abort', cancel);
    await cancellation;
  }
}

export const nativeLocalModel = {
  async state(signal?: AbortSignal) {
    available(signal);
    const state = await invoke<unknown>('local_model_state');
    signal?.throwIfAborted();
    return decodeLocalModelState(state);
  },
  async inspect(signal?: AbortSignal) {
    available(signal);
    return operation(
      'inspect_local_model',
      crypto.randomUUID(),
      decodeLocalModelPackageState,
      signal,
    );
  },
  async download(onProgress?: (value: LocalModelProgress) => void, signal?: AbortSignal) {
    const state = await nativeLocalModel.state(signal);
    const operationId = crypto.randomUUID();
    const stopped = new AbortController();
    const combined = signal ? AbortSignal.any([signal, stopped.signal]) : stopped.signal;
    let last: LocalModelProgress | undefined;
    let listening = true;
    const unlisten = await listen<unknown>('local-model-progress', ({ payload }) => {
      if (
        !listening ||
        combined.aborted ||
        !payload ||
        typeof payload !== 'object' ||
        (payload as Record<string, unknown>).operationId !== operationId
      )
        return;
      try {
        const progress = decodeLocalModelProgress(payload, operationId);
        if (
          progress.bytesVerified > state.packageBytes ||
          (last &&
            (progress.bytesVerified < last.bytesVerified ||
              progress.artifactsVerified < last.artifactsVerified))
        )
          throw new Error('Прогресс проверки модели не согласован с её пакетом.');
        last = progress;
        onProgress?.(progress);
      } catch (error) {
        stopped.abort(error);
      }
    });
    try {
      return await operation(
        'download_local_model',
        operationId,
        (value) => {
          const result = decodeLocalModelProgress(value, operationId);
          if (result.artifactsVerified !== 3 || result.bytesVerified !== state.packageBytes)
            throw new Error('Пакет локальной модели проверен не полностью.');
          return result;
        },
        combined,
      );
    } finally {
      listening = false;
      unlisten();
    }
  },
  async unload(signal?: AbortSignal) {
    available(signal);
    const result = await invoke<unknown>('unload_local_model');
    signal?.throwIfAborted();
    if (typeof result !== 'boolean') throw new Error('Неверный ответ освобождения модели.');
    return result;
  },
};
