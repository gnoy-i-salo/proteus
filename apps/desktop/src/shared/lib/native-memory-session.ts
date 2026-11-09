import { invoke } from '@tauri-apps/api/core';
import type { OpenResearchSessionRequest, ResearchSessionDescriptor } from '@proteus-int/contracts';
import { isDesktop } from './window-controls';
import { runNativeOperation } from './native-operation';

function available(signal?: AbortSignal): void {
  if (!isDesktop()) throw new Error('Исследование доступно в настольном приложении PROTEUS.');
  signal?.throwIfAborted();
}
async function closeHandle(sessionId: string): Promise<void> {
  const result = await invoke<unknown>('close_memory_session', { sessionId });
  if (typeof result !== 'boolean') throw new Error('Native не подтвердил закрытие сессии.');
}
async function discardAdmission(value: unknown): Promise<void> {
  if (!value || typeof value !== 'object' || !('id' in value)) return;
  const id = value.id;
  if (typeof id === 'string' && /^memorySession:[a-zA-Z0-9:_-]{1,114}$/u.test(id))
    await closeHandle(id);
}

export async function openNativeMemorySession<T extends ResearchSessionDescriptor>(
  command: string,
  request: OpenResearchSessionRequest,
  decode: (value: unknown) => T,
  signal?: AbortSignal,
): Promise<T> {
  available(signal);
  let raw: unknown;
  try {
    const session = await runNativeOperation(
      async () => {
        raw = await invoke<unknown>(command, { request });
        return raw;
      },
      decode,
      async () => {
        await invoke('cancel_ingestion_operation', { operationId: request.operationId });
        // This command returns void even before admission, so keep retrying
        // until the opening worker ends rather than assuming acknowledgment.
        return false;
      },
      signal,
    );
    signal?.throwIfAborted();
    return session;
  } catch (error) {
    await discardAdmission(raw);
    throw error;
  }
}

export function memorySessionTransport<T extends ResearchSessionDescriptor>(
  descriptor: T,
  onStop: () => void,
) {
  const session = Object.freeze(descriptor);
  let closed = false;
  let active: Promise<unknown> | undefined;
  let stopping: Promise<void> | undefined;

  function stop(): Promise<void> {
    closed = true;
    onStop();
    stopping ??= closeHandle(session.id).catch((error: unknown) => {
      stopping = undefined; // A transport failure may be retried by close/cancellation.
      throw error;
    });
    return stopping;
  }
  function check(signal?: AbortSignal): void {
    available(signal);
    if (closed) throw new Error('Исследовательская сессия закрыта.');
    if (active) throw new Error('Исследовательская сессия занята.');
  }
  async function execute<T>(
    command: string,
    args: Record<string, unknown>,
    decode: (value: unknown) => T,
    signal?: AbortSignal,
  ): Promise<T> {
    check(signal);
    let invalidResponse = false;
    const running = runNativeOperation(
      () => invoke<unknown>(command, { sessionId: session.id, ...args }),
      (value) => {
        if (closed) throw new Error('Исследовательская сессия закрыта.');
        try {
          return decode(value);
        } catch (error) {
          invalidResponse = true;
          throw error;
        }
      },
      async () => {
        await stop();
        return true;
      },
      signal,
    );
    active = running;
    try {
      const result = await running;
      signal?.throwIfAborted();
      if (closed) throw new Error('Исследовательская сессия закрыта.');
      return result;
    } finally {
      active = undefined;
      if (signal?.aborted || invalidResponse) await stop();
    }
  }
  return {
    session,
    check,
    execute,
    async close(this: void) {
      const running = active;
      const closing = stop();
      await Promise.allSettled([running, closing]);
      await closing;
    },
  };
}
