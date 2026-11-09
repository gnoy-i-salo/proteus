import {
  decodeOpenResearchSessionRequest,
  decodeResearchSession,
  decodeResearchRetrieval,
  decodeModelContextRequest,
  decodeResearchContext,
  decodeResearchPublication,
  decodeResearchCatalog,
  parseMemoryAction,
} from '@proteus-int/contracts';
import type {
  OpenResearchSessionRequest,
  ResearchMemoryPort,
  ResearchSessionDescriptor,
  RecallResult,
  SourceExcerpt,
} from '@proteus-int/contracts';
import {
  openNativeMemorySession,
  memorySessionTransport,
  exactMemoryInput,
} from './native-memory-session';

export async function openNativeResearch(
  request: OpenResearchSessionRequest,
  signal?: AbortSignal,
): Promise<ResearchMemoryPort> {
  const selected = decodeOpenResearchSessionRequest(request);
  const session = await openNativeMemorySession(
    'open_research_session',
    selected,
    (value) => decodeResearchSession(value, selected),
    signal,
  );
  return researchPort(selected, session);
}

function researchPort(
  request: OpenResearchSessionRequest,
  descriptor: ResearchSessionDescriptor,
): ResearchMemoryPort {
  const receipts = new Map<string, Pick<RecallResult, 'receipt' | 'sources'>>();
  const leases = new Set<string>();
  const { session, check, execute, close } = memorySessionTransport(descriptor, () => {
    receipts.clear();
    leases.clear();
  });
  function remember(result: Pick<RecallResult, 'receipt' | 'sources'>): void {
    if (receipts.has(result.receipt.id) || receipts.size >= request.recallBudget.maxCalls)
      throw new Error('Повторная или избыточная квитанция памяти.');
    receipts.set(result.receipt.id, structuredClone(result));
  }
  return {
    session,
    async getResearchCatalog(limit, signal) {
      check(signal);
      if (!Number.isSafeInteger(limit) || limit < 1 || limit > 10000)
        throw new Error('Неверный размер каталога исследования.');
      return execute(
        'research_memory_catalog',
        { limit },
        (value) => decodeResearchCatalog(value, session, limit),
        signal,
      );
    },
    async recall(query, intent, signal) {
      check(signal);
      const action = parseMemoryAction({ action: 'recall', query, intent });
      if (action.action !== 'recall') throw new Error('Неверное действие памяти.');
      exactMemoryInput(action.query, 4096);
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
      exactMemoryInput(action.episodeId, 256);
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
    close,
  };
}
