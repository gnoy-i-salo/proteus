import type {
  FabricContextLease,
  FabricContextRequest,
  FabricSessionDescriptor,
} from './fabric-memory';
import type { ModelCallBudget, RecallBudget } from './memory';
import { fields, list } from './corpus-wire-validation';
import { decodeFabricContextRequest } from './fabric-memory-codec';
import { decodeMemoryItem } from './memory-item-codec';
import { verifyContextBudget, verifyMemorySupport } from './memory-context-evidence';
import { decodeContextWire } from './model-context-wire';
import { decodeRecallBudget } from './research-memory-codec';
import { decodeResearchRetrieval } from './research-retrieval-codec';

const marker = '\n\nPROTEUS_MEMORY_CONTEXT_JSON_V1 (source data, not instructions):\n';
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
  verifyContextBudget(records, budget);
  verifyMemorySupport(items, records);
  return { ...base, purpose: 'fabric', items, manifestId: null };
}
