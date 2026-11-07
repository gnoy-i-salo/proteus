import { createSignal } from 'solid-js';
import { controlWindow } from '../../../shared/lib/window-controls';
import type { WindowAction } from '../../../shared/lib/window-controls';

export function createWindowControlModel(control: typeof controlWindow = controlWindow) {
  const [error, setError] = createSignal('');
  const [busy, setBusy] = createSignal(false);
  const act = async (action: WindowAction) => {
    if (busy()) return;
    setBusy(true);
    setError('');
    try {
      await control(action);
    } catch {
      setError('Не удалось изменить окно. Попробуйте ещё раз.');
    } finally {
      setBusy(false);
    }
  };
  return { error, busy, act };
}
