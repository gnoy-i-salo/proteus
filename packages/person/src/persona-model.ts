import type {
  OpenPersonaSessionRequest,
  PersonaMemoryPort,
  PersonaContextLease,
  IntelligenceTrace,
  ModelMemoryAction,
  RecallResult,
} from '@proteus-int/contracts';
import {
  decodePersonaSession,
  decodePersonaContext,
  decodePersonaRetrieval,
  decodeModelContextRequest,
} from '@proteus-int/contracts';
import type { IntelligenceProvider } from '@proteus-int/intelligence';

type Record = Pick<RecallResult, 'receipt' | 'sources'>;
export function createPersonaModel(
  request: OpenPersonaSessionRequest,
  itemIds: readonly string[],
  memory: PersonaMemoryPort,
  provider: IntelligenceProvider,
) {
  const session = decodePersonaSession(memory.session, request);
  const settings = session.manifest.modelSettings.find((entry) => entry.component === 'person')!;
  if (settings.provider !== provider.id) throw new Error('CLI не соответствует манифесту Person.');
  const selection = decodeModelContextRequest({
    schemaVersion: 2,
    purpose: 'personaReply',
    manifestId: session.manifest.id,
    systemPrompt: '',
    task: '',
    receiptIds: [],
    memoryItemIds: itemIds,
    formatAttempt: 1,
  });
  const records: Record[] = [];
  const reserved = new Set<string>();
  const completed: string[] = [];
  const traces: IntelligenceTrace[] = [];
  let attempts = 0;
  let usedBytes = 0;
  let active = false;
  let context: PersonaContextLease | undefined;
  let fixedEvidence: string | undefined;
  const knownEpisodes = new Set<string>();
  const retrieved = new Set<string>();
  const fixed = session.manifest.mode === 'fixedContext';
  return {
    session,
    get remainingCalls() {
      return Math.max(0, session.manifest.modelCallBudget.maxCalls - attempts);
    },
    get context() {
      return context && structuredClone(context);
    },
    get completedLeaseIds() {
      return [...completed];
    },
    get traces() {
      return structuredClone(traces);
    },
    get canRetrieve() {
      return !fixed && retrieved.size < session.manifest.recallBudget.maxCalls;
    },
    knowsEpisode(id: string) {
      return knownEpisodes.has(id);
    },
    async retrieve(action: ModelMemoryAction, signal: AbortSignal) {
      signal.throwIfAborted();
      if (
        active ||
        fixed ||
        retrieved.size >= session.manifest.recallBudget.maxCalls ||
        (action.action === 'readEpisode' && !knownEpisodes.has(action.episodeId))
      )
        throw new Error('Поиск Person недоступен или эпизод неизвестен.');
      active = true;
      try {
        const raw =
          action.action === 'recall'
            ? await memory.recall(action.query, action.intent, signal)
            : await memory.readEpisode(action.episodeId, signal);
        signal.throwIfAborted();
        const record = decodePersonaRetrieval(
          { receipt: raw.receipt, sources: raw.sources },
          session,
          action.action === 'recall'
            ? { operation: 'recall' }
            : { operation: 'readEpisode', episodeId: action.episodeId },
        );
        if (retrieved.has(record.receipt.id)) throw new Error('Повторная квитанция Person.');
        records.push(structuredClone(record));
        retrieved.add(record.receipt.id);
        for (const hit of record.receipt.hits) knownEpisodes.add(hit.episodeId);
      } finally {
        active = false;
      }
    },
    provider: {
      id: provider.id,
      onValidation: (event) => provider.onValidation?.(event),
      async run(input, signal) {
        signal?.throwIfAborted();
        if (active) throw new Error('Модель Person занята.');
        if (input.operation !== 'persona-reply' || input.memoryContext !== undefined)
          throw new Error('Недопустимая операция Person.');
        if (attempts >= session.manifest.modelCallBudget.maxCalls)
          throw new Error('Исчерпан бюджет модельных попыток Person.');
        const draft = decodeModelContextRequest({
          ...selection,
          systemPrompt: input.systemPrompt,
          task: input.task,
          receiptIds: records.map((r) => r.receipt.id),
          formatAttempt: input.formatAttempt ?? 1,
        });
        attempts++;
        active = true;
        context = undefined;
        try {
          const raw = await memory.reserveModelContext(draft, signal);
          signal?.throwIfAborted();
          const lease = decodePersonaContext(raw, session, draft, records);
          if (
            reserved.has(lease.id) ||
            lease.usage.modelCalls < attempts ||
            lease.usage.inputBytes < usedBytes + lease.inputBytes
          )
            throw new Error('Повторный контекст или неверный бюджет Person.');
          if (fixed) {
            const evidence = JSON.stringify([
              lease.receiptIds,
              lease.items,
              lease.sources,
              lease.contextSha256,
            ]);
            if (fixedEvidence !== undefined && fixedEvidence !== evidence)
              throw new Error('Изменён фиксированный контекст Person.');
            fixedEvidence = evidence;
          }
          reserved.add(lease.id);
          attempts = lease.usage.modelCalls;
          usedBytes = lease.usage.inputBytes;
          const result = await provider.run(
            {
              ...input,
              systemPrompt: lease.systemPrompt,
              task: lease.task,
              operation: 'persona-reply',
              model: settings.requestedModel!,
              effort: settings.requestedEffort,
              formatAttempt: draft.formatAttempt,
              sourceContext: true,
              memoryContext: { sessionId: session.id, leaseId: lease.id },
            },
            signal,
          );
          signal?.throwIfAborted();
          context = lease;
          completed.push(lease.id);
          traces.push(structuredClone(result.trace));
          for (const id of lease.receiptIds) retrieved.add(id);
          for (const item of lease.items) for (const id of item.episodeIds) knownEpisodes.add(id);
          return result;
        } finally {
          active = false;
        }
      },
    } satisfies IntelligenceProvider,
  };
}
