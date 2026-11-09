import { invoke } from '@tauri-apps/api/core';
import {
  decodeOpenResearchSessionRequest,
  decodeResearchSession,
  decodeResearchRetrieval,
  decodeModelContextRequest,
  decodeResearchContext,
  decodeResearchPublication,
  parseMemoryAction,
} from '@proteus-int/contracts';
import type {
  OpenResearchSessionRequest,
  ResearchMemoryPort,
  ResearchSessionDescriptor,
  RecallResult,
  SourceExcerpt,
} from '@proteus-int/contracts';
import { isDesktop } from './window-controls';
import { runNativeOperation } from './native-operation';

function available(signal?: AbortSignal): void {
  if (!isDesktop()) throw new Error('Исследование доступно в настольном приложении PROTEUS.');
  signal?.throwIfAborted();
}
function exactInput(value: string, maximumBytes: number): void {
  const bytes = new TextEncoder().encode(value);
  if (
    bytes.length > maximumBytes ||
    new TextDecoder('utf-8', { fatal: true, ignoreBOM: true }).decode(bytes) !== value
  )
    throw new Error('Неверный UTF-8 вход памяти.');
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

export async function openNativeResearch(
  request: OpenResearchSessionRequest,
  signal?: AbortSignal,
): Promise<ResearchMemoryPort> {
  available(signal);
  const selected = decodeOpenResearchSessionRequest(request);
  let raw: unknown;
  try {
    const session = await runNativeOperation(
      async () => {
        raw = await invoke<unknown>('open_research_session', { request: selected });
        return raw;
      },
      (value) => decodeResearchSession(value, selected),
      async () => {
        await invoke('cancel_ingestion_operation', { operationId: selected.operationId });
        // This command returns void even before admission, so keep retrying
        // until the opening worker ends rather than assuming acknowledgment.
        return false;
      },
      signal,
    );
    signal?.throwIfAborted();
    return researchPort(selected, session);
  } catch (error) {
    await discardAdmission(raw);
    throw error;
  }
}

function researchPort(
  request: OpenResearchSessionRequest,
  descriptor: ResearchSessionDescriptor,
): ResearchMemoryPort {
  const session = Object.freeze(descriptor);
  const receipts = new Map<string, Pick<RecallResult, 'receipt' | 'sources'>>();
  const leases = new Set<string>();
  let closed = false;
  let active: Promise<unknown> | undefined;
  let stopping: Promise<void> | undefined;

  function stop(): Promise<void> {
    closed = true;
    receipts.clear();
    leases.clear();
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
  function remember(result: Pick<RecallResult, 'receipt' | 'sources'>): void {
    if (receipts.has(result.receipt.id) || receipts.size >= request.recallBudget.maxCalls)
      throw new Error('Повторная или избыточная квитанция памяти.');
    receipts.set(result.receipt.id, structuredClone(result));
  }
  return {
    session,
    async recall(query, intent, signal) {
      check(signal);
      const action = parseMemoryAction({ action: 'recall', query, intent });
      if (action.action !== 'recall') throw new Error('Неверное действие памяти.');
      exactInput(action.query, 4096);
      return execute(
        'recall_memory',
        { queryText: action.query },
        (value) => {
          const result = decodeResearchRetrieval(value, session, request.recallBudget, {
            operation: 'recall',
          });
          remember(result);
          return { ...result, items: [], limitations: [] };
        },
        signal,
      );
    },
    async readEpisode(episodeId, signal) {
      check(signal);
      const action = parseMemoryAction({ action: 'readEpisode', episodeId });
      if (action.action !== 'readEpisode') throw new Error('Неверное действие памяти.');
      exactInput(action.episodeId, 256);
      if (/\p{Cc}/u.test(action.episodeId)) throw new Error('Неверный идентификатор эпизода.');
      return execute(
        'read_memory_episode',
        { episodeId: action.episodeId },
        (value) => {
          const result = decodeResearchRetrieval(value, session, request.recallBudget, {
            operation: 'readEpisode',
            episodeId: action.episodeId,
          });
          remember(result);
          return { ...result, episodeId: action.episodeId };
        },
        signal,
      );
    },
    async reserveModelContext(input, signal) {
      check(signal);
      const selected = decodeModelContextRequest(input);
      if (selected.purpose !== 'research')
        throw new Error('Эта сессия поддерживает только исследование.');
      const sources: SourceExcerpt[] = [];
      for (const id of selected.receiptIds) {
        const record = receipts.get(id);
        if (!record) throw new Error('Квитанция не принадлежит исследованию.');
        sources.push(...record.sources);
      }
      return execute(
        'reserve_memory_context',
        { request: selected },
        (value) => {
          const result = decodeResearchContext(
            value,
            session,
            selected,
            request.modelBudget,
            sources,
          );
          if (leases.has(result.id) || leases.size >= request.modelBudget.maxCalls)
            throw new Error('Повторный или избыточный контекст модели.');
          leases.add(result.id);
          return result;
        },
        signal,
      );
    },
    async publishResearch(input, signal) {
      check(signal);
      const draft = structuredClone(input);
      if (draft.contextLeaseIds.some((id) => !leases.has(id)))
        throw new Error('Контекст не принадлежит исследованию.');
      // Only native can prove successful CLI completion and atomically publish.
      return execute(
        'publish_memory_research',
        { draft },
        (value) => decodeResearchPublication(value, session, draft),
        signal,
      );
    },
    async close() {
      const running = active;
      const closing = stop();
      await Promise.allSettled([running, closing]);
      await closing;
    },
  };
}
