import type * as SolidClient from 'solid-js';
import { createRoot } from 'solid-js';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import type { ConnectionStatus, ImportedSource } from '@proteus-int/contracts';
import { createConnectionSession } from './connection-session';
import type { ConnectionSessionPort } from './connection-session';
import { createConnectionMaterials } from './connection-materials';
import type { ConnectionMaterialsPort } from './connection-materials';

// Exercise real client lifecycle behavior without rendering the application window.
vi.mock('solid-js', () => vi.importActual<typeof SolidClient>('solid-js/dist/solid.js'));

const qr: ConnectionStatus = {
  provider: 'telegram',
  status: 'qr-required',
  capabilities: [],
  qrCode: 'data:image/svg+xml;base64,PHN2Zy8+',
};
const disconnected: ConnectionStatus = {
  provider: 'telegram',
  status: 'disconnected',
  capabilities: [],
};
const imported: ImportedSource = {
  id: 'import:id',
  label: ' Exact\r\n👩🏽‍🔬 ',
  documentCount: 3,
  warnings: [],
};
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((complete) => {
    resolve = complete;
  });
  return { promise, resolve };
}
const tick = async () => {
  await Promise.resolve();
  await Promise.resolve();
};
beforeEach(() => {
  vi.useFakeTimers();
  vi.stubGlobal('window', globalThis);
});
afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});
function setup(statuses: readonly ConnectionStatus[] = [qr], desktop = true) {
  const port = {
    isDesktop: () => desktop,
    connectionStatuses: vi
      .fn<ConnectionSessionPort['connectionStatuses']>()
      .mockResolvedValue([...statuses]),
    configureConnection: vi
      .fn<ConnectionSessionPort['configureConnection']>()
      .mockResolvedValue(disconnected),
    beginConnection: vi.fn<ConnectionSessionPort['beginConnection']>().mockResolvedValue(qr),
    advanceConnection: vi
      .fn<ConnectionSessionPort['advanceConnection']>()
      .mockResolvedValue(disconnected),
    disconnectConnection: vi
      .fn<ConnectionSessionPort['disconnectConnection']>()
      .mockResolvedValue(disconnected),
    openConnectionPage: vi
      .fn<ConnectionSessionPort['openConnectionPage']>()
      .mockResolvedValue(undefined),
  };
  const materialsPort = {
    importSource: vi.fn<ConnectionMaterialsPort['importSource']>().mockResolvedValue(null),
    clearSourceData: vi
      .fn<ConnectionMaterialsPort['clearSourceData']>()
      .mockResolvedValue(undefined),
  };
  const onImported = vi.fn();
  const onDataCleared = vi.fn();
  const completed = vi.fn();
  let dispose!: () => void;
  const models = createRoot((stop) => {
    dispose = stop;
    const session = createConnectionSession(completed, port);
    const materials = createConnectionMaterials(
      session.selected,
      session.activity,
      { onImported, onDataCleared },
      materialsPort,
    );
    return { session, materials };
  });
  return { ...models, port, materialsPort, onImported, onDataCleared, completed, dispose };
}

it('polls only in desktop mode and clears the displayed QR when selection changes', async () => {
  const fixture = setup();
  await tick();
  expect(fixture.session.qrCode()).toBe(qr.qrCode);
  fixture.session.selectProvider('github');
  expect(fixture.session.current()).toMatchObject({ provider: 'github', status: 'unconfigured' });
  expect(fixture.session.qrCode()).toBeUndefined();
  fixture.session.selectProvider('telegram');
  expect(fixture.session.qrCode()).toBeUndefined();
  await vi.advanceTimersByTimeAsync(1500);
  expect(fixture.session.qrCode()).toBe(qr.qrCode);
  fixture.dispose();
  const browser = setup([], false);
  await vi.advanceTimersByTimeAsync(4500);
  expect(browser.port.connectionStatuses).not.toHaveBeenCalled();
  browser.dispose();
});

it('keeps polling serial and discards an old QR response after disconnect', async () => {
  const fixture = setup();
  await tick();
  const pending = deferred<ConnectionStatus[]>();
  fixture.port.connectionStatuses.mockReturnValueOnce(pending.promise);
  await vi.advanceTimersByTimeAsync(1500);
  await vi.advanceTimersByTimeAsync(3000);
  expect(fixture.port.connectionStatuses).toHaveBeenCalledTimes(2);
  await fixture.session.disconnect();
  expect(fixture.session.current().status).toBe('disconnected');
  pending.resolve([qr]);
  await tick();
  expect(fixture.session.current().status).toBe('disconnected');
  expect(fixture.session.qrCode()).toBeUndefined();
  fixture.dispose();
});

it('recovers from a failed poll without retaining its QR or exposing native error details', async () => {
  const fixture = setup();
  await tick();
  fixture.port.connectionStatuses.mockRejectedValueOnce('private-native-detail');
  await vi.advanceTimersByTimeAsync(1500);
  expect(fixture.session.error()).toBe(
    'Не удалось получить состояние подключений. Попробуйте ещё раз.',
  );
  expect(fixture.session.qrCode()).toBeUndefined();
  await vi.advanceTimersByTimeAsync(1500);
  expect(fixture.session.error()).toBe('');
  expect(fixture.session.qrCode()).toBe(qr.qrCode);
  fixture.dispose();
});

it('serializes actions, suppresses QR while busy and reports a safe action error', async () => {
  const fixture = setup();
  await tick();
  const pending = deferred<ConnectionStatus>();
  fixture.port.beginConnection.mockReturnValueOnce(pending.promise);
  const action = fixture.session.begin('phone');
  await fixture.session.disconnect();
  expect(fixture.port.disconnectConnection).not.toHaveBeenCalled();
  expect(fixture.session.busy()).toBe(true);
  expect(fixture.session.qrCode()).toBeUndefined();
  pending.resolve({ provider: 'telegram', status: 'phone-required', capabilities: [] });
  await action;
  expect(fixture.port.beginConnection).toHaveBeenCalledWith('telegram', 'phone');
  expect(fixture.completed).toHaveBeenCalledOnce();
  expect(fixture.session.busy()).toBe(false);
  fixture.port.disconnectConnection.mockRejectedValueOnce('private-native-detail');
  await fixture.session.disconnect();
  expect(fixture.session.error()).toBe(
    'Не удалось выполнить подключение. Проверьте данные и повторите попытку.',
  );
  fixture.dispose();
});

it('waits for the provider polling deadline before advancing GitHub login', async () => {
  const pending: ConnectionStatus = {
    provider: 'github',
    status: 'pending',
    pollAfterMs: 6000,
    capabilities: [],
  };
  const fixture = setup([pending]);
  await tick();
  fixture.session.selectProvider('github');
  fixture.port.advanceConnection.mockResolvedValue({
    provider: 'github',
    status: 'connected',
    capabilities: [],
  });
  await vi.advanceTimersByTimeAsync(4500);
  expect(fixture.port.advanceConnection).not.toHaveBeenCalled();
  await vi.advanceTimersByTimeAsync(1500);
  expect(fixture.port.advanceConnection).toHaveBeenCalledWith('github');
  expect(fixture.session.current().status).toBe('connected');
  fixture.dispose();
});

it('cleans up its timer and ignores late action results when the owner is disposed', async () => {
  const fixture = setup();
  await tick();
  const pending = deferred<ConnectionStatus>();
  fixture.port.beginConnection.mockReturnValueOnce(pending.promise);
  const action = fixture.session.begin('qr');
  fixture.dispose();
  pending.resolve(qr);
  await action;
  expect(fixture.completed).not.toHaveBeenCalled();
  expect(fixture.session.qrCode()).toBeUndefined();
  await vi.advanceTimersByTimeAsync(9000);
  expect(fixture.port.connectionStatuses).toHaveBeenCalledTimes(1);
});

it('keeps import metadata exact and requires an explicit second deletion action', async () => {
  const fixture = setup();
  await tick();
  fixture.session.selectProvider('instagram');
  fixture.materialsPort.importSource.mockResolvedValue(imported);
  await fixture.materials.pickImport();
  expect(fixture.onImported).toHaveBeenCalledWith('instagram', imported);
  expect(fixture.materials.imported()).toEqual(imported);
  await fixture.materials.clearData();
  expect(fixture.materials.clearConfirm()).toBe(true);
  expect(fixture.materialsPort.clearSourceData).not.toHaveBeenCalled();
  await fixture.materials.clearData();
  expect(fixture.materialsPort.clearSourceData).toHaveBeenCalledWith('instagram');
  expect(fixture.onDataCleared).toHaveBeenCalledWith('instagram');
  expect(fixture.materials.imported()).toBeUndefined();
  expect(fixture.materials.clearConfirm()).toBe(false);
  expect(fixture.materials.cleared()).toBe(true);
  fixture.materials.reset();
  expect(fixture.materials.cleared()).toBe(false);
  fixture.dispose();
});

it('preserves cancelled imports and ignores late files after disposal', async () => {
  const fixture = setup();
  await tick();
  await fixture.materials.pickImport();
  expect(fixture.materials.imported()).toBeUndefined();
  expect(fixture.onImported).not.toHaveBeenCalled();
  const pending = deferred<ImportedSource | null>();
  fixture.materialsPort.importSource.mockReturnValueOnce(pending.promise);
  const importing = fixture.materials.pickImport();
  await fixture.materials.clearData();
  expect(fixture.materials.clearConfirm()).toBe(false);
  fixture.dispose();
  pending.resolve(imported);
  await importing;
  expect(fixture.onImported).not.toHaveBeenCalled();
});

it('invalidates an old status poll when material import begins', async () => {
  const fixture = setup();
  await tick();
  const pending = deferred<ConnectionStatus[]>();
  fixture.port.connectionStatuses.mockReturnValueOnce(pending.promise);
  await vi.advanceTimersByTimeAsync(1500);
  await fixture.materials.pickImport();
  pending.resolve([qr]);
  await tick();
  expect(fixture.session.qrCode()).toBeUndefined();
  fixture.dispose();
});
