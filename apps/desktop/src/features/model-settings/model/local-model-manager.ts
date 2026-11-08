import { createSignal, onCleanup } from 'solid-js';
import { nativeLocalModel } from '../../../shared/lib/local-model';
import { isDesktop } from '../../../shared/lib/window-controls';
import type { LocalModelState, LocalModelProgress } from '../../../shared/lib/local-model-wire';

export function createLocalModelManager(port = nativeLocalModel, desktop = isDesktop()) {
  const [state, setState] = createSignal<LocalModelState>();
  const [packageState, setPackageState] = createSignal<'unknown' | 'missing' | 'ready'>('unknown');
  const [busy, setBusy] = createSignal<
    '' | 'refreshing' | 'checking' | 'downloading' | 'unloading'
  >('');
  const [stopping, setStopping] = createSignal(false);
  const [message, setMessage] = createSignal('');
  const [progress, setProgress] = createSignal<LocalModelProgress>();
  let active: AbortController | undefined;
  let disposed = false;
  const current = (controller: AbortController) => !disposed && !controller.signal.aborted;
  const fail = (error: unknown) => {
    if (!disposed) setMessage(error instanceof Error ? error.message : String(error));
  };
  onCleanup(() => {
    disposed = true;
    active?.abort(new Error('Настройки закрыты.'));
  });
  const refreshState = async (controller: AbortController) => {
    const value = await port.state(controller.signal);
    if (current(controller)) setState(value);
  };
  const run = async (
    action: Exclude<ReturnType<typeof busy>, ''>,
    work: (controller: AbortController) => Promise<void>,
  ) => {
    if (disposed || active) return;
    if (!desktop) {
      setMessage('Локальная модель доступна в настольном приложении PROTEUS.');
      return;
    }
    const controller = new AbortController();
    active = controller;
    setBusy(action);
    setStopping(false);
    setMessage('');
    setProgress(undefined);
    try {
      await work(controller);
    } catch (error) {
      if (current(controller)) fail(error);
    } finally {
      active = undefined;
      if (!disposed) {
        setBusy('');
        setStopping(false);
        if (controller.signal.aborted)
          setMessage('Операция отменена. Проверенные файлы сохранены.');
      }
    }
  };
  return {
    state,
    packageState,
    busy,
    stopping,
    message,
    progress,
    fail,
    refresh: () => run('refreshing', refreshState),
    inspect: () =>
      run('checking', async (controller) => {
        setPackageState('unknown');
        const value = await port.inspect(controller.signal);
        if (!current(controller)) return;
        setPackageState(value);
        setMessage(value === 'ready' ? 'Пакет модели проверен.' : 'Файлы модели ещё не загружены.');
        await refreshState(controller);
      }),
    download: () =>
      run('downloading', async (controller) => {
        setPackageState('unknown');
        await port.download((value) => {
          if (current(controller)) setProgress(value);
        }, controller.signal);
        if (!current(controller)) return;
        setPackageState('ready');
        setMessage('Пакет модели загружен и проверен.');
        await refreshState(controller);
      }),
    unload: () =>
      run('unloading', async (controller) => {
        const unloaded = await port.unload(controller.signal);
        if (!current(controller)) return;
        setMessage(
          unloaded
            ? 'Память модели освобождена; файлы сохранены.'
            : 'Модель уже выгружена из памяти.',
        );
        await refreshState(controller);
      }),
    cancel: () => {
      if (!active || disposed || active.signal.aborted) return;
      setStopping(true);
      setMessage('Ожидаем остановки операции…');
      active.abort(new Error('Операция отменена.'));
    },
  };
}
