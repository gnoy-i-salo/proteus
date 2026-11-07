import { beforeEach, expect, it, vi } from 'vitest';
import { exportPreset, importPreset, listenPresets } from './presets';

const native = vi.hoisted(() => ({
  desktop: true,
  invoke: vi.fn(),
  listen: vi.fn(),
  unlisten: vi.fn(),
  event: undefined as (() => void) | undefined,
}));
vi.mock('@tauri-apps/api/core', () => ({ invoke: native.invoke }));
vi.mock('@tauri-apps/api/event', () => ({ listen: native.listen }));
vi.mock('./window-controls', () => ({ isDesktop: () => native.desktop }));

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
  vi.clearAllMocks();
  native.desktop = true;
  native.event = undefined;
  native.invoke.mockResolvedValue([]);
  native.listen.mockImplementation(async (_name, receive) => {
    native.event = receive;
    return native.unlisten;
  });
});

it('preserves file-picker cancellation and exact preset text', async () => {
  native.invoke.mockResolvedValue(null);
  expect(await importPreset()).toBeNull();
  native.invoke.mockResolvedValue('  exact\r\n👩🏽‍🔬 public-urls ');
  expect(await importPreset()).toBe('  exact\r\n👩🏽‍🔬 public-urls ');
});

it.each([undefined, 3, { data: 'not the native shape' }, 'я'.repeat(65537)])(
  'rejects invalid or oversized import responses (%#)',
  async (value) => {
    native.invoke.mockResolvedValue(value);
    await expect(importPreset()).rejects.toMatchObject({
      name: 'PresetFileError',
      code: 'invalidResponse',
    });
  },
);

it('does not invoke native file import or subscriptions in browser mode', async () => {
  native.desktop = false;
  await expect(importPreset()).rejects.toMatchObject({
    name: 'PresetFileError',
    code: 'nativeUnavailable',
  });
  const stop = await listenPresets(vi.fn(), vi.fn());
  stop();
  expect(native.invoke).not.toHaveBeenCalled();
  expect(native.listen).not.toHaveBeenCalled();
});

it('converts native failures to typed errors without serializing unexpected objects', async () => {
  native.invoke.mockRejectedValue('Не удалось открыть пресет.');
  await expect(importPreset()).rejects.toMatchObject({
    name: 'PresetFileError',
    code: 'nativeFailure',
    message: 'Не удалось открыть пресет.',
  });
  native.invoke.mockRejectedValue({ secret: 'not for UI' });
  await expect(importPreset()).rejects.toMatchObject({
    name: 'PresetFileError',
    code: 'nativeFailure',
    message: 'Не удалось открыть пресет.',
  });
});

it('accepts native save cancellation and paths, rejects invalid save responses', async () => {
  for (const result of [null, '/tmp/personas.proteus']) {
    native.invoke.mockResolvedValue(result);
    await expect(exportPreset('exact preset')).resolves.toBeUndefined();
    expect(native.invoke).toHaveBeenLastCalledWith('export_preset', { data: 'exact preset' });
  }
  native.invoke.mockResolvedValue(3);
  await expect(exportPreset('exact preset')).rejects.toMatchObject({ code: 'invalidResponse' });
});

it('delivers ordered native Ok/Err results through canonical callbacks and cleans up once', async () => {
  native.invoke.mockResolvedValue([
    { Ok: '  first\r\n' },
    { Err: 'Проверьте файл.' },
    { Ok: 'second' },
  ]);
  const receive = vi.fn();
  const fail = vi.fn();
  const stop = await listenPresets(receive, fail);
  expect(receive.mock.calls).toEqual([['  first\r\n'], ['second']]);
  expect(fail).toHaveBeenCalledWith(
    expect.objectContaining({
      name: 'PresetFileError',
      code: 'nativeFailure',
      message: 'Проверьте файл.',
    }),
  );
  stop();
  stop();
  expect(native.unlisten).toHaveBeenCalledOnce();
});

it.each([
  null,
  {},
  [null],
  [{}],
  [{ Ok: null }],
  [{ Err: 42 }],
  [{ Ok: 'before' }, { Err: 3 }],
  [{ Ok: 'valid', Err: 'mixed' }],
  [{ Ok: 'valid', extra: true }],
  [{ Ok: 'я'.repeat(65537) }],
  Array.from({ length: 17 }, () => ({ Ok: 'valid' })),
])('rejects malformed native queues before delivering any file (%#)', async (value) => {
  native.invoke.mockResolvedValue(value);
  const receive = vi.fn();
  const fail = vi.fn();
  const stop = await listenPresets(receive, fail);
  expect(receive).not.toHaveBeenCalled();
  expect(fail).toHaveBeenCalledOnce();
  expect(fail).toHaveBeenCalledWith(expect.objectContaining({ code: 'invalidResponse' }));
  stop();
});

it('serializes event bursts and drains again without duplicating files', async () => {
  const initial = deferred<unknown>();
  native.invoke.mockReturnValueOnce(initial.promise).mockResolvedValueOnce([{ Ok: 'next' }]);
  const receive = vi.fn();
  const fail = vi.fn();
  const subscribed = listenPresets(receive, fail);
  await tick();
  native.event?.();
  native.event?.();
  expect(native.invoke).toHaveBeenCalledTimes(1);
  initial.resolve([{ Ok: 'initial' }]);
  const stop = await subscribed;
  expect(native.invoke).toHaveBeenCalledTimes(2);
  expect(receive.mock.calls).toEqual([['initial'], ['next']]);
  expect(fail).not.toHaveBeenCalled();
  stop();
  native.event?.();
  await tick();
  expect(native.invoke).toHaveBeenCalledTimes(2);
});

it('keeps the subscription usable after a native read fails', async () => {
  native.invoke
    .mockRejectedValueOnce('Очередь недоступна.')
    .mockResolvedValueOnce([{ Ok: 'recovered' }]);
  const receive = vi.fn();
  const fail = vi.fn();
  const stop = await listenPresets(receive, fail);
  native.event?.();
  await tick();
  expect(receive).toHaveBeenCalledWith('recovered');
  expect(fail).toHaveBeenCalledOnce();
  stop();
});

it('stops late listener registration after owner cancellation without reading files', async () => {
  const registration = deferred<() => void>();
  native.listen.mockReturnValueOnce(registration.promise);
  const controller = new AbortController();
  const receive = vi.fn();
  const fail = vi.fn();
  const subscribed = listenPresets(receive, fail, controller.signal);
  controller.abort();
  registration.resolve(native.unlisten);
  const stop = await subscribed;
  stop();
  expect(native.unlisten).toHaveBeenCalledOnce();
  expect(native.invoke).not.toHaveBeenCalled();
  expect(receive).not.toHaveBeenCalled();
  expect(fail).not.toHaveBeenCalled();
});

it('suppresses in-flight files and errors after owner cancellation', async () => {
  const read = deferred<unknown>();
  native.invoke.mockReturnValueOnce(read.promise);
  const controller = new AbortController();
  const receive = vi.fn();
  const fail = vi.fn();
  const subscribed = listenPresets(receive, fail, controller.signal);
  await tick();
  controller.abort();
  read.resolve([{ Ok: 'Must not apply' }, { Err: 'Must not display' }]);
  const stop = await subscribed;
  stop();
  expect(native.unlisten).toHaveBeenCalledOnce();
  expect(receive).not.toHaveBeenCalled();
  expect(fail).not.toHaveBeenCalled();
});

it('does not register a subscription for an already disposed owner', async () => {
  const controller = new AbortController();
  controller.abort();
  const stop = await listenPresets(vi.fn(), vi.fn(), controller.signal);
  stop();
  expect(native.listen).not.toHaveBeenCalled();
  expect(native.invoke).not.toHaveBeenCalled();
});
it('releases abort handling after subscription registration fails', async () => {
  const controller = new AbortController();
  const removed = vi.spyOn(controller.signal, 'removeEventListener');
  native.listen.mockRejectedValueOnce('Нельзя подписаться.');
  await expect(listenPresets(vi.fn(), vi.fn(), controller.signal)).rejects.toMatchObject({
    name: 'PresetFileError',
    code: 'nativeFailure',
    message: 'Нельзя подписаться.',
  });
  expect(removed).toHaveBeenCalledWith('abort', expect.any(Function));
  expect(native.invoke).not.toHaveBeenCalled();
  controller.abort();
  expect(native.unlisten).not.toHaveBeenCalled();
});

it('drains an event queued during a failed read without waiting for another event', async () => {
  let reject!: (reason: unknown) => void;
  const pending = new Promise<never>((_resolve, fail) => {
    reject = fail;
  });
  native.invoke.mockReturnValueOnce(pending).mockResolvedValueOnce([{ Ok: 'new file' }]);
  const receive = vi.fn();
  const fail = vi.fn();
  const subscribed = listenPresets(receive, fail);
  await tick();
  native.event?.();
  reject('Очередь временно недоступна.');
  const stop = await subscribed;
  expect(native.invoke).toHaveBeenCalledTimes(2);
  expect(receive).toHaveBeenCalledWith('new file');
  expect(fail).toHaveBeenCalledOnce();
  stop();
});
