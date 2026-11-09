import type { FabricMemoryPort, OpenFabricSessionRequest } from '@proteus-int/contracts';
import {
  decodeOpenFabricSessionRequest,
  decodeFabricSession,
  decodeFabricContextRequest,
  decodeFabricContext,
} from '@proteus-int/contracts';
import { openNativeMemorySession, memorySessionTransport } from './native-memory-session';

export async function openNativeFabric(
  request: OpenFabricSessionRequest,
  signal?: AbortSignal,
): Promise<FabricMemoryPort> {
  const selected = decodeOpenFabricSessionRequest(request);
  const descriptor = await openNativeMemorySession(
    'open_fabric_session',
    selected,
    (value) => decodeFabricSession(value, selected),
    signal,
  );
  const leases = new Set<string>();
  const { session, check, execute, close } = memorySessionTransport(descriptor, () =>
    leases.clear(),
  );
  return {
    session,
    async reserveModelContext(input, signal) {
      check(signal);
      const request = decodeFabricContextRequest(input);
      return execute(
        'reserve_fabric_memory_context',
        { request },
        (value) => {
          const context = decodeFabricContext(
            value,
            session,
            request,
            selected.modelBudget,
            selected.recallBudget,
          );
          if (leases.has(context.id) || leases.size >= selected.modelBudget.maxCalls)
            throw new Error('Повторный или избыточный контекст Fabric.');
          leases.add(context.id);
          return context;
        },
        signal,
      );
    },
    close,
  };
}
