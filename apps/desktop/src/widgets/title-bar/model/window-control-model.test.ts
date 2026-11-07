import type * as SolidClient from 'solid-js';
import { expect, it, vi } from 'vitest';
import { createWindowControlModel } from './window-control-model';
import type { WindowAction } from '../../../shared/lib/window-controls';

vi.mock('solid-js', () => vi.importActual<typeof SolidClient>('solid-js/dist/solid.js'));

it('serializes window operations until the native command settles', async () => {
  let resolve!: () => void;
  const pending = new Promise<void>((complete) => {
    resolve = complete;
  });
  const control = vi.fn<(action: WindowAction) => Promise<void>>().mockReturnValue(pending);
  const model = createWindowControlModel(control);
  const first = model.act('fullscreen');
  expect(model.busy()).toBe(true);
  await model.act('close');
  expect(control).toHaveBeenCalledExactlyOnceWith('fullscreen');
  resolve();
  await first;
  expect(model.busy()).toBe(false);
  expect(model.error()).toBe('');
});

it('exposes the existing user-facing error and permits a clean retry', async () => {
  const control = vi
    .fn<(action: WindowAction) => Promise<void>>()
    .mockRejectedValueOnce(new Error('Native operation failed'))
    .mockResolvedValueOnce(undefined);
  const model = createWindowControlModel(control);
  await model.act('maximize');
  expect(model.error()).toBe('Не удалось изменить окно. Попробуйте ещё раз.');
  expect(model.busy()).toBe(false);
  await model.act('minimize');
  expect(model.error()).toBe('');
  expect(control.mock.calls).toEqual([['maximize'], ['minimize']]);
});
