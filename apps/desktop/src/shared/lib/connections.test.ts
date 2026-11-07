import { beforeEach, expect, it, vi } from 'vitest';
import {
  configureConnection,
  connectionStatuses,
  createSourceGrant,
  importSource,
  nativeSourcePort,
} from './connections';
const native = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: native.invoke }));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn() }));
vi.mock('./window-controls', () => ({ isDesktop: () => true }));
beforeEach(() => vi.clearAllMocks());
const wireResult = {
  snapshotId: 'snapshot-id',
  documents: [],
  cursor: null,
  limitations: [' exact\r\n'],
  coverage: {
    documents: 0,
    totalDocuments: null,
    bytes: 0,
    complete: false,
    truncated: false,
    notAnalyzed: 0,
    unavailable: [],
  },
};

it('uses canonical native connection requests and statuses', async () => {
  native.invoke.mockResolvedValue({
    provider: 'telegram',
    status: 'disconnected',
    apiId: 123,
    capabilities: [],
  });
  expect(
    await configureConnection({ provider: 'telegram', apiId: 123, apiHash: 'synthetic-only' }),
  ).toEqual({ provider: 'telegram', status: 'disconnected', apiId: 123, capabilities: [] });
  expect(native.invoke).toHaveBeenCalledWith('configure_connection', {
    request: {
      provider: 'telegram',
      apiId: 123,
      apiHash: 'synthetic-only',
    },
  });
  native.invoke.mockResolvedValue([
    { provider: 'vk', status: 'disconnected', clientId: 'client-id', capabilities: [] },
  ]);
  expect(await connectionStatuses()).toEqual([
    { provider: 'vk', status: 'disconnected', clientId: 'client-id', capabilities: [] },
  ]);
});
it('requires explicit valid source response fields and keeps action command names stable', async () => {
  native.invoke.mockResolvedValue(wireResult);
  const result = await nativeSourcePort.execute('grant-id', {
    action: 'read',
    documentIds: ['doc-id'],
  });
  expect(native.invoke).toHaveBeenCalledWith('source_action', {
    grantId: 'grant-id',
    action: { action: 'read', documentIds: ['doc-id'] },
  });
  expect(result).toMatchObject({
    snapshotId: 'snapshot-id',
    coverage: { totalDocuments: null, notAnalyzed: 0 },
    limitations: [' exact\r\n'],
  });
  native.invoke.mockResolvedValue({ ...wireResult, 'snapshot-id': 'mixed' });
  await expect(nativeSourcePort.execute('grant-id', { action: 'coverage' })).rejects.toThrow();
});
it('roundtrips issued grants without turning model refusal into consent', async () => {
  const request = {
    provider: 'telegram' as const,
    resourceId: 'resource-id',
    authorId: null,
    from: null,
    to: null,
    allowModelProcessing: false,
    operations: ['read' as const],
    budget: { maxSteps: 2, maxDocuments: 3, maxBytes: 1024, maxDurationMs: 1000 },
  };
  native.invoke.mockImplementation(async (_command, args) => ({
    ...args.request,
    id: 'grant-id',
    accountId: 'account-id',
    snapshotId: 'snapshot-id',
    expiresAt: 'old-date',
  }));
  expect(await createSourceGrant(request)).toEqual({
    ...request,
    id: 'grant-id',
    accountId: 'account-id',
    snapshotId: 'snapshot-id',
    expiresAt: 'old-date',
  });
  expect(native.invoke.mock.calls[0]?.[1].request).toHaveProperty('allowModelProcessing', false);
});
it('preserves cancelled imports and rejects malformed status lists', async () => {
  native.invoke.mockResolvedValue(null);
  expect(await importSource('telegram')).toBeNull();
  native.invoke.mockResolvedValue({
    id: 'import-id',
    label: ' Exact ',
    documentCount: 2,
    warnings: [],
  });
  expect(await importSource('telegram')).toEqual({
    id: 'import-id',
    label: ' Exact ',
    documentCount: 2,
    warnings: [],
  });
  await expect(connectionStatuses()).rejects.toThrow();
});
it('does not accept results when cancellation arrives after native action returns', async () => {
  const controller = new AbortController();
  native.invoke.mockImplementation(async (command) => {
    if (command === 'cancel_source') return;
    controller.abort();
    return wireResult;
  });
  await expect(
    nativeSourcePort.execute('grant-id', { action: 'coverage' }, controller.signal),
  ).rejects.toThrow();
  expect(native.invoke).toHaveBeenCalledWith('cancel_source', { grantId: 'grant-id' });
});
