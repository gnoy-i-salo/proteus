import type {
  FabricMemoryPort,
  OpenFabricSessionRequest,
  FabricContextRequest,
} from '@proteus-int/contracts';
import type { IntelligenceProvider } from '@proteus-int/intelligence';
import { createFabricModel } from './fabric-model';
import type { FabricModel } from './fabric-model';

export type OpenFabricMemory = (
  request: OpenFabricSessionRequest,
  signal?: AbortSignal,
) => Promise<FabricMemoryPort>;

/** Shared lifecycle for task-free fabrication and separate per-task adaptation. */
export async function withFabricSession<T>(
  request: OpenFabricSessionRequest,
  selection: Pick<FabricContextRequest, 'memorySnapshotId' | 'itemIds'>,
  provider: IntelligenceProvider,
  open: OpenFabricMemory,
  signal: AbortSignal | undefined,
  run: (model: FabricModel, memory: FabricMemoryPort, signal: AbortSignal) => Promise<T>,
): Promise<T> {
  if (provider.id !== request.model.provider) throw new Error('CLI не соответствует Fabric.');
  const controller = new AbortController();
  const cancel = () => controller.abort(signal?.reason);
  signal?.addEventListener('abort', cancel, { once: true });
  const timer = setTimeout(
    () => controller.abort(new Error('Истёк срок подготовки Fabric.')),
    Math.min(request.recallBudget.maxDurationMs, request.modelBudget.maxDurationMs),
  );
  let memory: FabricMemoryPort | undefined;
  let failure: { cause: unknown } | undefined;
  let result: T | undefined;
  try {
    memory = await open(request, controller.signal);
    controller.signal.throwIfAborted();
    const model = createFabricModel(request, selection, memory, provider);
    result = await run(model, memory, controller.signal);
  } catch (cause) {
    failure = { cause };
  }
  try {
    await memory?.close();
    if (!failure) controller.signal.throwIfAborted();
  } catch (cause) {
    if (failure)
      throw new AggregateError(
        [failure.cause, cause],
        'Подготовка и закрытие Fabric завершились ошибкой.',
        { cause },
      );
    throw cause;
  } finally {
    clearTimeout(timer);
    signal?.removeEventListener('abort', cancel);
  }
  if (failure) throw failure.cause;
  return result!;
}
