import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { nativeLocalModel } from './local-model';
import fixture from './fixtures/local-model-wire.json';
const host = vi.hoisted(() => ({
  desktop: true,
  invoke: vi.fn<(command: string, args?: Record<string, unknown>) => Promise<unknown>>(),
  listen:
    vi.fn<(name: string, handler: (event: { payload: unknown }) => void) => Promise<() => void>>(),
}));
vi.mock('@tauri-apps/api/core', () => ({ invoke: host.invoke }));
vi.mock('@tauri-apps/api/event', () => ({ listen: host.listen }));
vi.mock('./window-controls', () => ({ isDesktop: () => host.desktop }));
const operationId = '00000000-0000-4000-8000-000000000001';
const complete = { ...fixture.progress, operationId };
let receive: (event: { payload: unknown }) => void;
const unlisten = vi.fn();
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: Error) => void;
  const promise = new Promise<T>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}
beforeEach(() => {
  vi.resetAllMocks();
  host.desktop = true;
  vi.spyOn(crypto, 'randomUUID').mockReturnValue(operationId);
  host.listen.mockImplementation((_name, handler) => {
    receive = handler;
    return Promise.resolve(unlisten);
  });
  host.invoke.mockImplementation((command) => {
    if (command === 'local_model_state') return Promise.resolve(fixture.state);
    if (command === 'download_local_model') return Promise.resolve(complete);
    if (command === 'inspect_local_model') return Promise.resolve('ready');
    return Promise.resolve(false);
  });
});
afterEach(() => {
  vi.restoreAllMocks();
  vi.useRealTimers();
});
it('decodes native state, disk inspection and unload without exposing paths', async () => {
  expect(await nativeLocalModel.state()).toEqual(fixture.state);
  expect(await nativeLocalModel.inspect()).toBe('ready');
  expect(await nativeLocalModel.unload()).toBe(false);
  expect(host.invoke.mock.calls).toEqual([
    ['local_model_state'],
    ['inspect_local_model', { operationId }],
    ['unload_local_model'],
  ]);
});
it('never invokes native outside desktop or after pre-cancellation', async () => {
  host.desktop = false;
  for (const run of [
    () => nativeLocalModel.state(),
    () => nativeLocalModel.inspect(),
    () => nativeLocalModel.download(),
    () => nativeLocalModel.unload(),
  ])
    await expect(run()).rejects.toThrow('настольном');
  host.desktop = true;
  const signal = AbortSignal.abort(new Error('already stopped'));
  for (const run of [
    () => nativeLocalModel.state(signal),
    () => nativeLocalModel.inspect(signal),
    () => nativeLocalModel.download(undefined, signal),
    () => nativeLocalModel.unload(signal),
  ])
    await expect(run()).rejects.toThrow('already stopped');
  expect(host.invoke).not.toHaveBeenCalled();
  expect(host.listen).not.toHaveBeenCalled();
});
it('subscribes before download, validates its final receipt and removes the listener', async () => {
  host.invoke.mockImplementation((command) => {
    if (command === 'local_model_state') return Promise.resolve(fixture.state);
    expect(host.listen).toHaveBeenCalledExactlyOnceWith(
      'local-model-progress',
      expect.any(Function),
    );
    return Promise.resolve(complete);
  });
  expect(await nativeLocalModel.download()).toEqual(complete);
  expect(host.invoke).toHaveBeenCalledWith('download_local_model', { operationId });
  expect(unlisten).toHaveBeenCalledOnce();
});
it.each([
  { ...complete, artifactsVerified: 2 },
  { ...complete, bytesVerified: complete.bytesVerified - 1 },
  { ...complete, operationId: 'foreign' },
  { ...complete, schemaVersion: 2 },
])('rejects incomplete or foreign completion %j', async (bad) => {
  host.invoke.mockImplementation((command) =>
    Promise.resolve(command === 'local_model_state' ? fixture.state : bad),
  );
  await expect(nativeLocalModel.download()).rejects.toThrow();
  expect(unlisten).toHaveBeenCalledOnce();
});
it('delivers only matching verified progress and ignores foreign events', async () => {
  const onProgress = vi.fn();
  host.invoke.mockImplementation((command) => {
    if (command === 'local_model_state') return Promise.resolve(fixture.state);
    receive({ payload: { ...complete, operationId: 'foreign' } });
    receive({ payload: { ...complete, artifactsVerified: 1, bytesVerified: 100 } });
    receive({ payload: complete });
    return Promise.resolve(complete);
  });
  await nativeLocalModel.download(onProgress);
  expect(onProgress.mock.calls).toEqual([
    [{ ...complete, artifactsVerified: 1, bytesVerified: 100 }],
    [complete],
  ]);
});
it.each(['backward', 'oversize', 'malformed', 'callback'])(
  'stops a download after %s progress',
  async (kind) => {
    host.invoke.mockImplementation((command) => {
      if (command === 'local_model_state') return Promise.resolve(fixture.state);
      if (command === 'cancel_local_model_operation') return Promise.resolve(true);
      receive({ payload: complete });
      if (kind !== 'callback')
        receive({
          payload:
            kind === 'malformed'
              ? { operationId }
              : {
                  ...complete,
                  bytesVerified: kind === 'oversize' ? complete.bytesVerified + 1 : 1,
                },
        });
      return Promise.resolve(complete);
    });
    await expect(
      nativeLocalModel.download(() => {
        if (kind === 'callback') throw new Error('callback failed');
      }),
    ).rejects.toThrow();
    expect(host.invoke).toHaveBeenCalledWith('cancel_local_model_operation', { operationId });
    expect(unlisten).toHaveBeenCalledOnce();
  },
);
it('retries cancellation that arrives before native worker admission without overlapping calls', async () => {
  vi.useFakeTimers();
  const started = deferred<void>();
  const worker = deferred<unknown>();
  const firstCancel = deferred<boolean>();
  let cancellations = 0;
  host.invoke.mockImplementation((command) => {
    if (command === 'local_model_state') return Promise.resolve(fixture.state);
    if (command === 'download_local_model') {
      started.resolve();
      return worker.promise;
    }
    cancellations += 1;
    return cancellations === 1 ? firstCancel.promise : Promise.resolve(true);
  });
  const controller = new AbortController();
  const response = nativeLocalModel.download(undefined, controller.signal);
  const rejected = expect(response).rejects.toThrow('stop now');
  await started.promise;
  controller.abort(new Error('stop now'));
  await vi.advanceTimersByTimeAsync(1000);
  expect(cancellations).toBe(1);
  firstCancel.resolve(false);
  await vi.advanceTimersByTimeAsync(100);
  expect(cancellations).toBe(2);
  await vi.advanceTimersByTimeAsync(1000);
  expect(cancellations).toBe(2);
  worker.resolve(complete);
  await rejected;
  expect(vi.getTimerCount()).toBe(0);
  expect(unlisten).toHaveBeenCalledOnce();
});
it('retries a failed early cancellation and keeps waiting for native completion', async () => {
  vi.useFakeTimers();
  const started = deferred<void>();
  const worker = deferred<unknown>();
  let attempts = 0;
  host.invoke.mockImplementation((command) => {
    if (command === 'inspect_local_model') {
      started.resolve();
      return worker.promise;
    }
    attempts += 1;
    return attempts === 1 ? Promise.reject(new Error('not admitted')) : Promise.resolve(true);
  });
  const controller = new AbortController();
  const response = nativeLocalModel.inspect(controller.signal);
  const rejected = expect(response).rejects.toThrow('stop inspection');
  await started.promise;
  controller.abort(new Error('stop inspection'));
  await vi.advanceTimersByTimeAsync(100);
  expect(attempts).toBe(2);
  worker.reject(new Error('native stopped'));
  await rejected;
  expect(vi.getTimerCount()).toBe(0);
});
it('cleans up if cancellation happens during listener setup without starting a download', async () => {
  const controller = new AbortController();
  host.listen.mockImplementation(() => {
    controller.abort(new Error('stop setup'));
    return Promise.resolve(unlisten);
  });
  await expect(nativeLocalModel.download(undefined, controller.signal)).rejects.toThrow(
    'stop setup',
  );
  expect(host.invoke.mock.calls.map(([command]) => command)).toEqual(['local_model_state']);
  expect(unlisten).toHaveBeenCalledOnce();
});
it('does not start downloading if the listener or state validation fails', async () => {
  host.listen.mockRejectedValue(new Error('listener unavailable'));
  await expect(nativeLocalModel.download()).rejects.toThrow('listener unavailable');
  host.invoke.mockResolvedValue({ ...fixture.state, schemaVersion: 2 });
  await expect(nativeLocalModel.download()).rejects.toThrow();
  expect(host.invoke.mock.calls.every(([command]) => command === 'local_model_state')).toBe(true);
});
it('rejects malformed inspection and unload responses', async () => {
  host.invoke.mockResolvedValue('yes');
  await expect(nativeLocalModel.inspect()).rejects.toThrow();
  await expect(nativeLocalModel.unload()).rejects.toThrow();
});
