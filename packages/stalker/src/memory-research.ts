import type {
  IntelligenceTrace,
  OpenResearchSessionRequest,
  ResearchMemoryPort,
  ResearchPublicationReceipt,
  ModelMemoryAction,
  MemoryItemDraft,
} from '@proteus-int/contracts';
import { parseMemoryAction } from '@proteus-int/contracts';
import type { IntelligenceProvider } from '@proteus-int/intelligence';
import { structured } from '@proteus-int/intelligence';
import { selectResearchSamples, validateResearchInput } from './research-plan';
import type { MemoryResearchInput } from './research-plan';
import { researchBatches, assignDraftIds } from './research-batches';
import type { ResearchRecord } from './research-batches';
import { createResearchModel } from './research-model';
import type { ResearchModel } from './research-model';
import { parseMemoryAnalysis } from './parse-memory-analysis';
import { researchPlanningInstructions, memoryExtractionInstructions } from './research-prompts';
export interface MemoryResearchResult {
  readonly publication: ResearchPublicationReceipt;
  readonly traces: readonly IntelligenceTrace[];
  readonly limitations: readonly string[];
}
export type OpenResearchMemory = (
  request: OpenResearchSessionRequest,
  signal?: AbortSignal,
) => Promise<ResearchMemoryPort>;

function planningAction(
  value: Record<string, unknown>,
  knownEpisodes: ReadonlySet<string>,
): ModelMemoryAction | { action: 'finish'; reason: string } {
  if (value.action === 'finish') {
    if (
      Object.keys(value).length !== 2 ||
      typeof value.reason !== 'string' ||
      !value.reason.trim() ||
      new TextEncoder().encode(value.reason).length > 1000
    )
      throw new Error('Неверное завершение исследования.');
    return { action: 'finish', reason: value.reason };
  }
  const action = parseMemoryAction(value);
  if (action.action === 'readEpisode' && !knownEpisodes.has(action.episodeId))
    throw new Error('Модель запросила неизвестный эпизод.');
  return action;
}

async function planFollowups(
  input: MemoryResearchInput,
  memory: ResearchMemoryPort,
  model: ResearchModel,
  records: ResearchRecord[],
  limitations: string[],
  signal: AbortSignal,
): Promise<void> {
  const seen = new Set<string>();
  for (let step = 0; step < input.limits.maxPlanningSteps; step++) {
    signal.throwIfAborted();
    if (model.remainingCalls < 4 || records.length >= input.request.recallBudget.maxCalls) {
      limitations.push(
        'Поиск дополнительных оснований ограничен бюджетом; попытки сохранены для извлечения памяти.',
      );
      return;
    }
    const batches = researchBatches(records, input.limits.maxItemsPerBatch).batches;
    const context = batches.at(-1)?.receiptIds ?? [];
    const known = new Set(
      records.flatMap((record) => record.receipt.hits.map((hit) => hit.episodeId)),
    );
    const { data: action } = await structured(
      model.forReceipts(context),
      'STALKER memory planning',
      researchPlanningInstructions,
      {
        person: input.person,
        acquisition: {
          loadedDocuments: input.snapshot.coverage.loadedDocuments,
          indexedDocuments: input.snapshot.coverage.indexedDocuments,
          totalDocuments: input.snapshot.coverage.totalDocuments,
          complete: input.snapshot.coverage.complete,
        },
        knownEpisodeIds: [...known].slice(0, 100),
      },
      signal,
      input.request.model.effort,
      (value) => planningAction(value, known),
      'corpus-analysis',
      true,
    );
    if (action.action === 'finish') return;
    const key = JSON.stringify(action);
    if (seen.has(key)) {
      limitations.push('Повторяющийся поисковый шаг остановлен.');
      return;
    }
    seen.add(key);
    const record =
      action.action === 'recall'
        ? await memory.recall(action.query, action.intent, signal)
        : await memory.readEpisode(action.episodeId, signal);
    signal.throwIfAborted();
    records.push({ receipt: record.receipt, sources: record.sources });
  }
  if (input.limits.maxPlanningSteps > 0)
    limitations.push(
      'Предел поисковых шагов достигнут; дополнительные основания могли остаться вне выборки.',
    );
}
async function extractMemory(
  input: MemoryResearchInput,
  model: ResearchModel,
  records: readonly ResearchRecord[],
  limitations: string[],
  signal: AbortSignal,
): Promise<readonly MemoryItemDraft[]> {
  const selected = researchBatches(records, input.limits.maxItemsPerBatch);
  if (selected.skipped)
    limitations.push(
      `Фрагменты за пределами размера модельного контекста не анализировались: ${selected.skipped}.`,
    );
  const items: MemoryItemDraft[] = [];
  for (const [index, batch] of selected.batches.entries()) {
    signal.throwIfAborted();
    if (model.remainingCalls < 2 || items.length >= 1000) {
      limitations.push(
        'Извлечение памяти ограничено бюджетом; оставшиеся фрагменты не анализировались.',
      );
      break;
    }
    const scope = { ...batch.scope, maxItems: Math.min(batch.scope.maxItems, 1000 - items.length) };
    const { data } = await structured(
      model.forReceipts(batch.receiptIds),
      'STALKER memory extraction',
      memoryExtractionInstructions,
      { person: input.person, maxItems: scope.maxItems },
      signal,
      input.request.model.effort,
      (value) => parseMemoryAnalysis(value, scope),
      'corpus-analysis',
      true,
    );
    items.push(...assignDraftIds(data, index));
  }
  return items;
}

export async function researchMemory(
  input: MemoryResearchInput,
  provider: IntelligenceProvider,
  open: OpenResearchMemory,
  signal?: AbortSignal,
): Promise<MemoryResearchResult> {
  signal?.throwIfAborted();
  const selected = validateResearchInput(input);
  if (provider.id !== selected.request.model.provider)
    throw new Error('CLI не соответствует исследованию.');
  const controller = new AbortController();
  const cancel = () => controller.abort(signal?.reason);
  signal?.addEventListener('abort', cancel, { once: true });
  const timer = setTimeout(
    () => controller.abort(new Error('Истёк срок исследования.')),
    Math.min(
      selected.request.recallBudget.maxDurationMs,
      selected.request.modelBudget.maxDurationMs,
    ),
  );
  let memory: ResearchMemoryPort | undefined;
  let failure: { cause: unknown } | undefined;
  let result: MemoryResearchResult | undefined;
  try {
    memory = await open(selected.request, controller.signal);
    controller.signal.throwIfAborted();
    const model = createResearchModel(selected.request, memory, provider);
    const records: ResearchRecord[] = [];
    const limitations: string[] = [];
    const samples = selectResearchSamples(selected);
    if (samples.length < selected.snapshot.episodeIds.length)
      limitations.push('Начальная выборка охватывает лишь часть эпизодов снимка.');
    for (const sample of samples) {
      const record = await memory.readEpisode(sample.episodeId, controller.signal);
      controller.signal.throwIfAborted();
      records.push({ receipt: record.receipt, sources: record.sources });
    }
    await planFollowups(selected, memory, model, records, limitations, controller.signal);
    const items = await extractMemory(selected, model, records, limitations, controller.signal);
    controller.signal.throwIfAborted();
    const publication = await memory.publishResearch(
      {
        schemaVersion: 1,
        extractionVersion: 'stalker-memory:1',
        items,
        analyzedRefs: model.analyzedRefs,
        contextLeaseIds: model.completedLeaseIds,
      },
      controller.signal,
    );
    controller.signal.throwIfAborted();
    result = { publication, traces: model.traces, limitations: [...new Set(limitations)] };
  } catch (cause) {
    failure = { cause };
  } finally {
    clearTimeout(timer);
    signal?.removeEventListener('abort', cancel);
  }
  try {
    await memory?.close();
  } catch (cause) {
    if (failure)
      throw new AggregateError(
        [failure.cause, cause],
        'Исследование и закрытие сессии завершились ошибкой.',
        { cause },
      );
    throw cause;
  }
  if (failure) throw failure.cause;
  return result!;
}
