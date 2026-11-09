import type {
  FabricMemoryPort,
  OpenFabricSessionRequest,
  RevisionReference,
} from '@proteus-int/contracts';
import {
  decodeOpenFabricSessionRequest,
  decodeFabricSession,
  decodeFabricContextRequest,
  decodeFabricContext,
  decodePersonaManifestDraft,
  decodeManifestFreeze,
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
  // Keep only immutable reference metadata, never another copy of source text or prompts.
  const leases = new Map<
    string,
    { memorySnapshotId: string; sourceRefs: readonly RevisionReference[] }
  >();
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
          const sourceRefs = new Map<string, RevisionReference>();
          for (const { span } of context.sources) {
            const reference = { documentId: span.documentId, revisionId: span.revisionId };
            sourceRefs.set(JSON.stringify(reference), reference);
          }
          leases.set(context.id, {
            memorySnapshotId: request.memorySnapshotId,
            sourceRefs: [...sourceRefs.values()],
          });
          return context;
        },
        signal,
      );
    },
    async freezeManifest(input, signal) {
      check(signal);
      const draft = decodePersonaManifestDraft(input);
      const references = new Map<string, RevisionReference>();
      for (const id of draft.contextLeaseIds) {
        const lease = leases.get(id);
        if (!lease || lease.memorySnapshotId !== draft.memorySnapshotId)
          throw new Error('Контекст не принадлежит выбранной памяти Fabric.');
        for (const reference of lease.sourceRefs)
          references.set(JSON.stringify(reference), reference);
      }
      // Local identity checks do not prove a successful CLI call. Native verifies
      // completion and live permissions before committing or replaying the bundle.
      return execute(
        'freeze_memory_manifest',
        { draft },
        (value) => decodeManifestFreeze(value, session, draft, [...references.values()]),
        signal,
      );
    },
    close,
  };
}
