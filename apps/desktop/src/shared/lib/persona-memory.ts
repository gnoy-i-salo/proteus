import type {
  OpenPersonaSessionRequest,
  PersonaMemoryPort,
  RecallResult,
} from '@proteus-int/contracts';
import {
  decodeOpenPersonaSessionRequest,
  decodePersonaSession,
  decodeModelContextRequest,
  decodePersonaContext,
  decodePersonaRetrieval,
  parseMemoryAction,
  decodeAppendPersonaSimulationRequest,
  decodePersonaSimulation,
} from '@proteus-int/contracts';
import {
  openNativeMemorySession,
  memorySessionTransport,
  exactMemoryInput,
} from './native-memory-session';

export async function openNativePersona(
  request: OpenPersonaSessionRequest,
  signal?: AbortSignal,
): Promise<PersonaMemoryPort> {
  const selected = decodeOpenPersonaSessionRequest(request);
  const descriptor = await openNativeMemorySession(
    'open_persona_session',
    selected,
    (value) => decodePersonaSession(value, selected),
    signal,
  );
  const records = new Map<string, Pick<RecallResult, 'receipt' | 'sources'>>();
  const leases = new Set<string>();
  let fixedSelection: string | undefined;
  let fixedEvidence: string | undefined;
  let usedCalls = 0;
  let usedBytes = 0;
  const { session, check, execute, close } = memorySessionTransport(descriptor, () => {
    records.clear();
    leases.clear();
    fixedSelection = undefined;
    fixedEvidence = undefined;
  });
  const fixed = session.manifest.mode === 'fixedContext';
  function actionAllowed(signal?: AbortSignal): void {
    check(signal);
    if (fixed) throw new Error('Поиск недоступен в режиме фиксированного контекста.');
  }
  function remember(record: Pick<RecallResult, 'receipt' | 'sources'>): void {
    if (records.has(record.receipt.id) || records.size >= session.manifest.recallBudget.maxCalls)
      throw new Error('Повторная или избыточная квитанция Person.');
    records.set(record.receipt.id, structuredClone(record));
  }
  return {
    // The native descriptor is private: callers may edit their detached copy,
    // but cannot change budgets, model bindings or the source scope of this port.
    get session() {
      return Object.freeze(structuredClone(session));
    },
    async recall(query, intent, signal) {
      actionAllowed(signal);
      const action = parseMemoryAction({ action: 'recall', query, intent });
      if (action.action !== 'recall') throw new Error('Неверное действие Person.');
      exactMemoryInput(action.query, 4096);
      return execute(
        'recall_memory',
        { queryText: action.query },
        (value) => {
          const result = decodePersonaRetrieval(value, session, { operation: 'recall' });
          remember(result);
          return { ...result, items: [], limitations: [] };
        },
        signal,
      );
    },
    async readEpisode(episodeId, signal) {
      actionAllowed(signal);
      const action = parseMemoryAction({ action: 'readEpisode', episodeId });
      if (action.action !== 'readEpisode') throw new Error('Неверное действие Person.');
      exactMemoryInput(action.episodeId, 256);
      if (/\p{Cc}/u.test(action.episodeId)) throw new Error('Неверный идентификатор эпизода.');
      return execute(
        'read_memory_episode',
        { episodeId: action.episodeId },
        (value) => {
          const result = decodePersonaRetrieval(value, session, {
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
      const request = decodeModelContextRequest(input);
      const selection = JSON.stringify(request.memoryItemIds);
      if (
        request.purpose !== 'personaReply' ||
        request.manifestId !== session.manifest.id ||
        (fixed &&
          (request.receiptIds.length !== 0 ||
            (fixedSelection !== undefined && fixedSelection !== selection)))
      )
        throw new Error('Контекст не соответствует сессии Person.');
      const prior = request.receiptIds.map((id) => {
        const record = records.get(id);
        if (!record) throw new Error('Квитанция не принадлежит сессии Person.');
        return record;
      });
      return execute(
        'reserve_memory_context',
        { request },
        (value) => {
          const result = decodePersonaContext(value, session, request, prior);
          if (
            leases.has(result.id) ||
            leases.size >= session.manifest.modelCallBudget.maxCalls ||
            result.usage.modelCalls <= usedCalls ||
            result.usage.inputBytes < usedBytes + result.inputBytes
          )
            throw new Error('Повторный контекст или неверный накопленный бюджет Person.');
          if (fixed) {
            const evidence = JSON.stringify([
              result.receiptIds,
              result.items,
              result.sources,
              result.contextSha256,
            ]);
            if (fixedEvidence !== undefined && fixedEvidence !== evidence)
              throw new Error('Изменён фиксированный контекст Person.');
            fixedSelection = selection;
            fixedEvidence = evidence;
          }
          leases.add(result.id);
          usedCalls = result.usage.modelCalls;
          usedBytes = result.usage.inputBytes;
          return result;
        },
        signal,
      );
    },
    async appendSimulation(input, signal) {
      check(signal);
      const request = decodeAppendPersonaSimulationRequest(input);
      if (!leases.has(request.contextLeaseId))
        throw new Error('Контекст симуляции не принадлежит сессии Person.');
      return execute(
        'append_persona_simulation',
        { request },
        (value) => decodePersonaSimulation(value, session.manifest.id, request),
        signal,
      );
    },
    close,
  };
}
