import type {
  OpenResearchSessionRequest,
  ResearchMemoryPort,
  IntelligenceTrace,
  RevisionReference,
} from '@proteus-int/contracts';
import type { IntelligenceProvider } from '@proteus-int/intelligence';
import {
  decodeOpenResearchSessionRequest,
  decodeResearchSession,
  decodeModelContextRequest,
} from '@proteus-int/contracts';

export interface ResearchModel {
  readonly remainingCalls: number;
  readonly completedLeaseIds: readonly string[];
  readonly traces: readonly IntelligenceTrace[];
  readonly analyzedRefs: readonly RevisionReference[];
  forReceipts(receiptIds: readonly string[]): IntelligenceProvider;
}

export function createResearchModel(
  request: OpenResearchSessionRequest,
  memory: ResearchMemoryPort,
  provider: IntelligenceProvider,
): ResearchModel {
  const selected = decodeOpenResearchSessionRequest(request);
  const session = decodeResearchSession(memory.session, selected);
  if (provider.id !== selected.model.provider)
    throw new Error('CLI не соответствует исследовательской сессии.');
  let attempts = 0;
  const completed: string[] = [];
  const traces: IntelligenceTrace[] = [];
  const analyzed = new Map<string, RevisionReference>();
  return {
    get remainingCalls() {
      return Math.max(0, selected.modelBudget.maxCalls - attempts);
    },
    get completedLeaseIds() {
      return [...completed];
    },
    get traces() {
      return structuredClone(traces);
    },
    get analyzedRefs() {
      return [...analyzed.values()].map((ref) => ({ ...ref }));
    },
    forReceipts(receiptIds) {
      const selectedReceipts = [...receiptIds];
      return {
        id: provider.id,
        onValidation: (event) => provider.onValidation?.(event),
        async run(input, signal) {
          signal?.throwIfAborted();
          if (input.operation !== 'corpus-analysis' || input.memoryContext !== undefined)
            throw new Error('Недопустимая операция исследовательской модели.');
          if (attempts >= selected.modelBudget.maxCalls)
            throw new Error('Исчерпан бюджет модельных попыток исследования.');
          const context = decodeModelContextRequest({
            schemaVersion: 2,
            purpose: 'research',
            manifestId: null,
            systemPrompt: input.systemPrompt,
            task: input.task,
            receiptIds: selectedReceipts,
            memoryItemIds: [],
            formatAttempt: input.formatAttempt ?? 1,
          });
          // Conservatively account for a failed reservation/transport too. Native
          // owns actual cumulative bytes, deadlines, permissions and dispatch.
          attempts++;
          const lease = await memory.reserveModelContext(context, signal);
          attempts = Math.max(attempts, lease.usage.modelCalls);
          signal?.throwIfAborted();
          const result = await provider.run(
            {
              ...input,
              systemPrompt: lease.systemPrompt,
              task: lease.task,
              model: selected.model.model,
              effort: selected.model.effort,
              operation: 'corpus-analysis',
              formatAttempt: context.formatAttempt,
              sourceContext: true,
              memoryContext: { sessionId: session.id, leaseId: lease.id },
            },
            signal,
          );
          signal?.throwIfAborted();
          completed.push(lease.id);
          traces.push(structuredClone(result.trace));
          for (const { span } of lease.sources) {
            const ref = { documentId: span.documentId, revisionId: span.revisionId };
            analyzed.set(JSON.stringify([ref.documentId, ref.revisionId]), ref);
          }
          return result;
        },
      };
    },
  };
}
