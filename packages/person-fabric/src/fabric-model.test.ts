import { beforeEach, expect, it, vi } from 'vitest';
import type { FabricMemoryPort, IntelligenceRequest } from '@proteus-int/contracts';
import type { IntelligenceProvider } from '@proteus-int/intelligence';
import { structured } from '@proteus-int/intelligence';
import { createFabricModel } from './fabric-model';
import { request, selection, session, lease } from './testing/fabric-fixtures';

const base: IntelligenceRequest = {
  systemPrompt: '\uFEFFProfile instructions\r\n',
  task: '{"person":"Анна"}',
  operation: 'generation',
  effort: 'low',
  formatAttempt: 1,
};
const trace = { provider: 'codex' as const, reasoningMs: 1, terminalMs: 2, summary: 'done' };
const run = vi.fn<IntelligenceProvider['run']>();
const reserve = vi.fn<FabricMemoryPort['reserveModelContext']>();
const onValidation = vi.fn<NonNullable<IntelligenceProvider['onValidation']>>();
const provider: IntelligenceProvider = { id: 'codex', run, onValidation };
const memory: FabricMemoryPort = {
  session,
  reserveModelContext: reserve,
  freezeManifest: vi.fn(),
  close: vi.fn(),
};
beforeEach(() => {
  vi.resetAllMocks();
  let calls = 0;
  reserve.mockImplementation((input) => Promise.resolve(lease(input, ++calls)));
  run.mockResolvedValue({ answer: '{"skills":[]}', trace });
});
it('dispatches native context with fixed settings and exposes only successfully completed memory', async () => {
  const model = createFabricModel(request, selection, memory, provider);
  expect(model.context).toBeUndefined();
  await model.provider.run(base);
  const context = {
    schemaVersion: 1 as const,
    ...selection,
    systemPrompt: base.systemPrompt,
    task: base.task,
    formatAttempt: 1 as const,
  };
  expect(reserve).toHaveBeenCalledExactlyOnceWith(context, undefined);
  expect(run).toHaveBeenCalledExactlyOnceWith(
    {
      ...base,
      task: lease(context).task,
      model: 'bound-model',
      effort: 'high',
      sourceContext: true,
      memoryContext: { sessionId: session.id, leaseId: 'lease:1' },
    },
    undefined,
  );
  expect(model.context?.items).toEqual(lease(context).items);
  expect(model.completedLeaseIds).toEqual(['lease:1']);
  expect(model.remainingCalls).toBe(2);
  expect(model.traces).toEqual([trace]);
  Object.assign(model.context!.items[0]!, { text: 'changed' });
  Object.assign(model.traces[0]!, { summary: 'changed' });
  expect(model.context?.items).toEqual(lease(context).items);
  expect(model.traces).toEqual([trace]);
});
it('counts repair attempts, obtains fresh leases and forwards validation events', async () => {
  const model = createFabricModel(request, selection, memory, provider);
  run.mockResolvedValueOnce({ answer: 'not JSON', trace });
  await structured(model.provider, 'FABRIC', 'Profile without future task', { person: 'Анна' });
  expect(reserve.mock.calls.map(([input]) => input.formatAttempt)).toEqual([1, 2]);
  expect(model.completedLeaseIds).toEqual(['lease:1', 'lease:2']);
  expect(model.context?.id).toBe('lease:2');
  expect(onValidation.mock.calls.map(([event]) => event.status)).toEqual(['rejected', 'accepted']);
});
it('counts failed reservation and CLI attempts without exposing stale completion data', async () => {
  const model = createFabricModel(request, selection, memory, provider);
  await model.provider.run(base);
  reserve.mockRejectedValueOnce(new Error('grant revoked'));
  await expect(model.provider.run(base)).rejects.toThrow('grant revoked');
  expect(model.context).toBeUndefined();
  run.mockRejectedValueOnce(new Error('CLI failed'));
  await expect(model.provider.run(base)).rejects.toThrow('CLI failed');
  await expect(model.provider.run(base)).rejects.toThrow('бюджет');
  expect(model.completedLeaseIds).toEqual(['lease:1']);
  expect(model.traces).toEqual([trace]);
  expect(run).toHaveBeenCalledTimes(2);
});
it('rejects foreign session/provider/selection and non-generation operations before reservation', async () => {
  expect(() =>
    createFabricModel(request, selection, memory, { ...provider, id: 'kimi' }),
  ).toThrow();
  expect(() =>
    createFabricModel(
      request,
      selection,
      { ...memory, session: { ...session, personaId: 'foreign' } },
      provider,
    ),
  ).toThrow();
  expect(() =>
    createFabricModel(request, { ...selection, itemIds: ['x', 'x'] }, memory, provider),
  ).toThrow();
  const model = createFabricModel(request, selection, memory, provider);
  await expect(model.provider.run({ ...base, operation: 'corpus-analysis' })).rejects.toThrow();
  await expect(
    model.provider.run({ ...base, memoryContext: { sessionId: 'forged', leaseId: 'forged' } }),
  ).rejects.toThrow();
  expect(reserve).not.toHaveBeenCalled();
});
it('does not call the provider after cancelled reservation or record late model success', async () => {
  const controller = new AbortController();
  reserve.mockImplementationOnce((input) => {
    controller.abort(new Error('stop reservation'));
    return Promise.resolve(lease(input));
  });
  const model = createFabricModel(request, selection, memory, provider);
  await expect(model.provider.run(base, controller.signal)).rejects.toThrow('stop reservation');
  expect(run).not.toHaveBeenCalled();
  const second = new AbortController();
  run.mockImplementationOnce(() => {
    second.abort(new Error('late answer'));
    return Promise.resolve({ answer: '{}', trace });
  });
  await expect(model.provider.run(base, second.signal)).rejects.toThrow('late answer');
  expect(model.context).toBeUndefined();
  expect(model.completedLeaseIds).toEqual([]);
});

it('rejects malformed or repeated leases before dispatch and keeps selection detached', async () => {
  const chosen = { ...selection, itemIds: [...selection.itemIds] };
  const model = createFabricModel(request, chosen, memory, provider);
  chosen.itemIds[0] = 'foreign';
  await model.provider.run(base);
  reserve.mockImplementationOnce((input) => Promise.resolve(lease(input, 1)));
  await expect(model.provider.run(base)).rejects.toThrow('Повторный');
  reserve.mockImplementationOnce((input) =>
    Promise.resolve({ ...lease(input, 3), personaId: 'foreign' }),
  );
  await expect(model.provider.run(base)).rejects.toThrow();
  expect(reserve.mock.calls.every(([input]) => input.itemIds[0] === 'item-1')).toBe(true);
  expect(run).toHaveBeenCalledTimes(1);
  expect(model.completedLeaseIds).toEqual(['lease:1']);
});
it('serializes model calls without consuming another attempt while one is active', async () => {
  const model = createFabricModel(request, selection, memory, provider);
  let release!: () => void;
  const ready = new Promise<void>((resolve) => {
    release = resolve;
  });
  run.mockImplementationOnce(async () => {
    await ready;
    return { answer: '{}', trace };
  });
  const first = model.provider.run(base);
  await expect(model.provider.run(base)).rejects.toThrow('занята');
  release();
  await first;
  expect(model.remainingCalls).toBe(2);
  expect(reserve).toHaveBeenCalledTimes(1);
});
