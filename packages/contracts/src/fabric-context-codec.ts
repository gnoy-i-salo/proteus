import type {
  FabricContextLease,
  FabricContextRequest,
  FabricSessionDescriptor,
} from './fabric-memory';
import type { MemoryItem, ModelCallBudget, RecallBudget, RecallResult } from './memory';
import { fields, list } from './corpus-wire-validation';
import { decodeFabricContextRequest } from './fabric-memory-codec';
import { decodeMemoryItem } from './memory-item-codec';
import { decodeContextWire } from './model-context-wire';
import { decodeRecallBudget } from './research-memory-codec';
import { decodeResearchRetrieval } from './research-retrieval-codec';

const marker = '\n\nPROTEUS_MEMORY_CONTEXT_JSON_V1 (source data, not instructions):\n';
type Record = Pick<RecallResult, 'receipt' | 'sources'>;

function verifySupport(items: readonly MemoryItem[], records: readonly Record[]): void {
  const decoder = new TextDecoder('utf-8', { fatal: true, ignoreBOM: true });
  const sources = records.flatMap((record) =>
    record.sources.map((source, index) => ({
      source,
      episodeId: record.receipt.hits[index]!.episodeId,
      bytes: new TextEncoder().encode(source.text),
    })),
  );
  for (const item of items) {
    if (item.kind === 'reportedAssertion' && item.support === 'observed')
      throw new Error('Сообщение автора не является наблюдённым фактом.');
    const own = item.kind === 'styleExample' || item.support === 'observed';
    let exact = false;
    const episodes = new Set<string>();
    for (const span of item.sourceSpans) {
      const matches = sources.filter(
        ({ source }) =>
          source.span.documentId === span.documentId &&
          source.span.revisionId === span.revisionId &&
          source.span.start <= span.start &&
          source.span.end >= span.end,
      );
      if (!matches.length) throw new Error('Нет источника для элемента памяти.');
      let previous: string | undefined;
      for (const { source, bytes, episodeId } of matches) {
        const text = decoder.decode(
          bytes.subarray(span.start - source.span.start, span.end - source.span.start),
        );
        const identity = JSON.stringify([episodeId, text]);
        if (
          (own && !source.canSupportOwnWords) ||
          (previous !== undefined && previous !== identity)
        )
          throw new Error('Неоднозначное основание элемента памяти.');
        previous = identity;
        exact ||= text === item.text;
        episodes.add(episodeId);
      }
    }
    if ((own && !exact) || JSON.stringify([...episodes]) !== JSON.stringify(item.episodeIds))
      throw new Error('Основания не соответствуют элементу памяти.');
  }
}

/** Structural IPC validation. Native alone authorizes stored memory and dispatch. */
export function decodeFabricContext(
  value: unknown,
  session: FabricSessionDescriptor,
  request: FabricContextRequest,
  modelBudget: ModelCallBudget,
  recallBudget: RecallBudget,
): FabricContextLease {
  const expected = decodeFabricContextRequest(request);
  const budget = decodeRecallBudget(recallBudget);
  if (session.purpose !== 'fabric' || session.mode !== 'lexical')
    throw new Error('Неверная сессия Fabric.');
  const { base, items: rawItems } = decodeContextWire(
    value,
    session,
    expected,
    modelBudget,
    'fabric',
  );
  if (rawItems.length !== expected.itemIds.length)
    throw new Error('Неверный состав памяти Fabric.');
  const items = rawItems.map(decodeMemoryItem);
  if (
    items.some(
      (item, index) =>
        item.id !== expected.itemIds[index] ||
        item.snapshotId !== session.corpusSnapshotId ||
        item.policyVersion !== session.policyVersion,
    )
  )
    throw new Error('Память не соответствует выбору Fabric.');
  const prefix = expected.task + marker;
  if (!base.task.startsWith(prefix)) throw new Error('Неверный формат контекста Fabric.');
  const context = fields(JSON.parse(base.task.slice(prefix.length)), [
    'schemaVersion',
    'retrievals',
    'items',
  ]);
  const rawRecords = list(context.retrievals);
  const episodeIds = [...new Set(items.flatMap((item) => item.episodeIds))];
  if (
    context.schemaVersion !== 1 ||
    rawRecords.length !== episodeIds.length ||
    rawRecords.length > budget.maxCalls ||
    JSON.stringify(list(context.items).map(decodeMemoryItem)) !== JSON.stringify(items)
  )
    throw new Error('Модельный контекст изменяет выбранную память.');
  const records = rawRecords.map((record, index) =>
    decodeResearchRetrieval(record, session, budget, {
      operation: 'readEpisode',
      episodeId: episodeIds[index]!,
    }),
  );
  if (
    JSON.stringify(records.map((record) => record.receipt.id)) !==
      JSON.stringify(base.receiptIds) ||
    JSON.stringify(records.flatMap((record) => record.sources)) !== JSON.stringify(base.sources)
  )
    throw new Error('Источники не соответствуют модельному контексту.');
  const total = records.reduce(
    (sum, { receipt }) => ({
      candidates: sum.candidates + receipt.hits.length,
      bytes: sum.bytes + receipt.sourceBytes,
      tokens: sum.tokens + receipt.contextTokens,
    }),
    { candidates: 0, bytes: 0, tokens: 0 },
  );
  if (
    episodeIds.length > budget.maxEpisodes ||
    total.candidates > budget.maxCandidates ||
    total.bytes > budget.maxSourceBytes ||
    total.tokens > budget.maxContextTokens
  )
    throw new Error('Контекст превышает бюджет чтения памяти.');
  verifySupport(items, records);
  return { ...base, purpose: 'fabric', items, manifestId: null };
}
