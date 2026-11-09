import type {
  FabricMemoryPort,
  OpenFabricSessionRequest,
  FabricContextRequest,
  FabricContextLease,
  IntelligenceTrace,
} from '@proteus-int/contracts';
import {
  decodeOpenFabricSessionRequest,
  decodeFabricSession,
  decodeFabricContextRequest,
  decodeFabricContext,
} from '@proteus-int/contracts';
import type { IntelligenceProvider } from '@proteus-int/intelligence';

export interface FabricModel {
  readonly provider: IntelligenceProvider;
  readonly remainingCalls: number;
  readonly completedLeaseIds: readonly string[];
  readonly context: FabricContextLease | undefined;
  readonly traces: readonly IntelligenceTrace[];
}

export function createFabricModel(
  request: OpenFabricSessionRequest,
  selection: Pick<FabricContextRequest, 'memorySnapshotId' | 'itemIds'>,
  memory: FabricMemoryPort,
  provider: IntelligenceProvider,
): FabricModel {
  const selected = decodeOpenFabricSessionRequest(request);
  const session = decodeFabricSession(memory.session, selected);
  const scope = decodeFabricContextRequest({
    schemaVersion: 1,
    memorySnapshotId: selection.memorySnapshotId,
    itemIds: selection.itemIds,
    systemPrompt: '',
    task: '',
    formatAttempt: 1,
  });
  if (provider.id !== selected.model.provider)
    throw new Error('CLI не соответствует сессии Fabric.');
  let attempts = 0;
  let active = false;
  let context: FabricContextLease | undefined;
  const reserved = new Set<string>();
  const completed: string[] = [];
  const traces: IntelligenceTrace[] = [];
  return {
    get remainingCalls() {
      return Math.max(0, selected.modelBudget.maxCalls - attempts);
    },
    get completedLeaseIds() {
      return [...completed];
    },
    get context() {
      return context && structuredClone(context);
    },
    get traces() {
      return structuredClone(traces);
    },
    provider: {
      id: provider.id,
      onValidation: (event) => provider.onValidation?.(event),
      async run(input, signal) {
        signal?.throwIfAborted();
        if (active) throw new Error('Модель Fabric занята.');
        if (input.operation !== 'generation' || input.memoryContext !== undefined)
          throw new Error('Недопустимая операция модели Fabric.');
        if (attempts >= selected.modelBudget.maxCalls)
          throw new Error('Исчерпан бюджет модельных попыток Fabric.');
        const draft = decodeFabricContextRequest({
          ...scope,
          systemPrompt: input.systemPrompt,
          task: input.task,
          formatAttempt: input.formatAttempt ?? 1,
        });
        // Failed reservations count conservatively too; native owns cumulative
        // bytes, deadlines, live grants and actual completion proof.
        attempts++;
        active = true;
        context = undefined;
        try {
          const raw = await memory.reserveModelContext(draft, signal);
          signal?.throwIfAborted();
          const lease = decodeFabricContext(
            raw,
            session,
            draft,
            selected.modelBudget,
            selected.recallBudget,
          );
          if (reserved.has(lease.id)) throw new Error('Повторный контекст Fabric.');
          reserved.add(lease.id);
          attempts = Math.max(attempts, lease.usage.modelCalls);
          const result = await provider.run(
            {
              ...input,
              systemPrompt: lease.systemPrompt,
              task: lease.task,
              operation: 'generation',
              model: selected.model.model,
              effort: selected.model.effort,
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
          return result;
        } finally {
          active = false;
        }
      },
    },
  };
}
