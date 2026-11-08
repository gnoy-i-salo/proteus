import type * as SolidClient from 'solid-js';
import { createRoot } from 'solid-js';
import { expect, it, vi } from 'vitest';
import { createLocalModelManager } from './local-model-manager';
import type { nativeLocalModel } from '../../../shared/lib/local-model';
import type { LocalModelState } from '../../../shared/lib/local-model-wire';
vi.mock('solid-js', () => vi.importActual<typeof SolidClient>('solid-js/dist/solid.js'));
const state: LocalModelState = {
  schemaVersion: 1,
  modelId: 'intfloat/multilingual-e5-small',
  modelRevision: 'a'.repeat(40),
  packageBytes: 100,
  runtimeLoaded: false,
  operationActive: false,
};
function fixture(desktop = true) {
  const port = {
    state: vi.fn<typeof nativeLocalModel.state>().mockResolvedValue(state),
    inspect: vi.fn<typeof nativeLocalModel.inspect>().mockResolvedValue('missing'),
    download: vi.fn<typeof nativeLocalModel.download>().mockResolvedValue({
      schemaVersion: 1,
      operationId: 'op',
      artifactsVerified: 3,
      bytesVerified: 100,
    }),
    unload: vi.fn<typeof nativeLocalModel.unload>().mockResolvedValue(true),
  };
  return createRoot((dispose) => ({
    port,
    dispose,
    model: createLocalModelManager(port, desktop),
  }));
}
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<T>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}
it('does not download or inspect files on construction or metadata refresh', async () => {
  const { model, port, dispose } = fixture();
  expect(port.state).not.toHaveBeenCalled();
  await model.refresh();
  expect(model.state()).toEqual(state);
  expect(model.packageState()).toBe('unknown');
  expect(port.inspect).not.toHaveBeenCalled();
  expect(port.download).not.toHaveBeenCalled();
  dispose();
});
it('keeps browser preview explicit without calling native', async () => {
  const { model, port, dispose } = fixture(false);
  await model.refresh();
  await model.inspect();
  await model.download();
  await model.unload();
  expect(model.message()).toContain('настольном');
  for (const call of Object.values(port)) expect(call).not.toHaveBeenCalled();
  dispose();
});
it('only declares disk readiness after native inspection or verified download', async () => {
  const { model, port, dispose } = fixture();
  await model.inspect();
  expect(model.packageState()).toBe('missing');
  port.inspect.mockResolvedValue('ready');
  await model.inspect();
  expect(model.packageState()).toBe('ready');
  port.inspect.mockRejectedValue(new Error('corrupt files'));
  await model.inspect();
  expect(model.packageState()).toBe('unknown');
  expect(model.message()).toContain('corrupt files');
  await model.download();
  expect(model.packageState()).toBe('ready');
  expect(model.message()).toContain('проверен');
  dispose();
});
it('holds busy until cancelled native work settles and suppresses late progress and success', async () => {
  const { model, port, dispose } = fixture();
  const pending = deferred<Awaited<ReturnType<typeof nativeLocalModel.download>>>();
  port.download.mockReturnValue(pending.promise);
  const run = model.download();
  expect(model.busy()).toBe('downloading');
  const [progress, signal] = port.download.mock.calls[0]!;
  progress?.({ schemaVersion: 1, operationId: 'op', artifactsVerified: 1, bytesVerified: 10 });
  expect(model.progress()?.bytesVerified).toBe(10);
  model.cancel();
  expect(signal?.aborted).toBe(true);
  expect(model.stopping()).toBe(true);
  await model.inspect();
  await model.download();
  await model.unload();
  expect(port.download).toHaveBeenCalledOnce();
  expect(port.inspect).not.toHaveBeenCalled();
  expect(port.unload).not.toHaveBeenCalled();
  progress?.({ schemaVersion: 1, operationId: 'op', artifactsVerified: 3, bytesVerified: 100 });
  expect(model.progress()?.bytesVerified).toBe(10);
  pending.resolve({
    schemaVersion: 1,
    operationId: 'op',
    artifactsVerified: 3,
    bytesVerified: 100,
  });
  await run;
  expect(model.busy()).toBe('');
  expect(model.stopping()).toBe(false);
  expect(model.packageState()).toBe('unknown');
  expect(model.message()).toContain('отменена');
  dispose();
});
it('can retry a failed operation without retaining obsolete progress', async () => {
  const { model, port, dispose } = fixture();
  port.download.mockRejectedValueOnce(new Error('offline'));
  await model.download();
  expect(model.message()).toContain('offline');
  expect(model.busy()).toBe('');
  await model.download();
  expect(model.message()).toContain('проверен');
  expect(model.progress()).toBeUndefined();
  dispose();
});
it('unloads only RAM and refreshes status without claiming disk files were deleted', async () => {
  const { model, port, dispose } = fixture();
  port.inspect.mockResolvedValue('ready');
  await model.inspect();
  await model.unload();
  expect(port.unload).toHaveBeenCalledOnce();
  expect(model.packageState()).toBe('ready');
  expect(model.state()?.runtimeLoaded).toBe(false);
  expect(model.message()).toContain('освобождена');
  dispose();
});
it('aborts on disposal and ignores late replies without invoking another native operation', async () => {
  const { model, port, dispose } = fixture();
  const pending = deferred<LocalModelState>();
  port.state.mockReturnValue(pending.promise);
  const run = model.refresh();
  const signal = port.state.mock.calls[0]![0];
  dispose();
  expect(signal?.aborted).toBe(true);
  pending.resolve(state);
  await run;
  expect(model.state()).toBeUndefined();
  await model.download();
  expect(port.download).not.toHaveBeenCalled();
});
it('refresh failure stays visible even after a successful download', async () => {
  const { model, port, dispose } = fixture();
  port.state.mockRejectedValue(new Error('status unavailable'));
  await model.download();
  expect(model.packageState()).toBe('ready');
  expect(model.message()).toContain('status unavailable');
  expect(model.state()).toBeUndefined();
  dispose();
});
