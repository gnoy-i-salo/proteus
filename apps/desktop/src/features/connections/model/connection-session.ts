import { createSignal, onCleanup, onMount } from 'solid-js';
import type {
  ConnectionConfig,
  ConnectionPage,
  ConnectionStatus,
  SourceProvider,
  TelegramAuthMethod,
} from '@proteus-int/contracts';
import {
  advanceConnection,
  beginConnection,
  configureConnection,
  connectionStatuses,
  disconnectConnection,
  openConnectionPage,
} from '../../../shared/lib/connections';
import { isDesktop } from '../../../shared/lib/window-controls';

export interface ConnectionSessionPort {
  readonly isDesktop: typeof isDesktop;
  readonly connectionStatuses: typeof connectionStatuses;
  readonly configureConnection: typeof configureConnection;
  readonly beginConnection: typeof beginConnection;
  readonly advanceConnection: typeof advanceConnection;
  readonly disconnectConnection: typeof disconnectConnection;
  readonly openConnectionPage: typeof openConnectionPage;
}
export interface ConnectionActivity {
  readonly busy: () => boolean;
  readonly start: () => boolean;
  readonly active: () => boolean;
  readonly fail: (message: string) => void;
  readonly finish: () => void;
}
export function createConnectionSession(
  onActionComplete: () => void,
  port: ConnectionSessionPort = {
    isDesktop,
    connectionStatuses,
    configureConnection,
    beginConnection,
    advanceConnection,
    disconnectConnection,
    openConnectionPage,
  },
) {
  const desktop = port.isDesktop();
  const [selected, setSelected] = createSignal<SourceProvider>('telegram');
  const [statuses, setStatuses] = createSignal<readonly ConnectionStatus[]>([]);
  const [busy, setBusy] = createSignal(false);
  const [actionError, setError] = createSignal('');
  const [pollError, setPollError] = createSignal('');
  const error = () => actionError() || pollError();
  let disposed = false;
  let revision = 0;
  let nextPoll = 0;
  let refreshing = false;
  const current = (): ConnectionStatus =>
    statuses().find((item) => item.provider === selected()) ?? {
      provider: selected(),
      status: 'unconfigured',
      capabilities: [],
    };
  const qrCode = () => {
    const value = current().qrCode;
    return selected() === 'telegram' &&
      current().status === 'qr-required' &&
      !busy() &&
      !error() &&
      value &&
      value.length <= 128 * 1024 &&
      /^data:image\/svg\+xml;base64,[A-Za-z0-9+/]+={0,2}$/u.test(value)
      ? value
      : undefined;
  };
  const withoutQr = (status: ConnectionStatus): ConnectionStatus => {
    const result = { ...status };
    delete result.qrCode;
    return result;
  };
  const visibleStatus = (status: ConnectionStatus): ConnectionStatus =>
    selected() === 'telegram' && status.provider === 'telegram' && status.status === 'qr-required'
      ? status
      : withoutQr(status);
  const clearQr = () => setStatuses((items) => items.map(withoutQr));

  const refresh = async () => {
    if (!desktop || busy() || refreshing) return;
    refreshing = true;
    const version = revision;
    try {
      const result = await port.connectionStatuses();
      if (!disposed && version === revision) {
        setStatuses(result.map(visibleStatus));
        setPollError('');
        const pending = result.find(
          (item) => item.provider === 'github' && item.status === 'pending',
        );
        if (pending && nextPoll === 0)
          nextPoll = Date.now() + Math.max(1000, pending.pollAfterMs ?? 5000);
      }
    } catch {
      if (!disposed && version === revision) {
        clearQr();
        setPollError('Не удалось получить состояние подключений. Попробуйте ещё раз.');
      }
    } finally {
      refreshing = false;
    }
  };
  const startAction = () => {
    if (busy()) return false;
    ++revision;
    clearQr();
    setBusy(true);
    setError('');
    setPollError('');
    return true;
  };
  const perform = async (
    operation: () => Promise<ConnectionStatus | void>,
    failureMessage = 'Не удалось выполнить подключение. Проверьте данные и повторите попытку.',
  ) => {
    if (!startAction()) return;
    const version = revision;
    try {
      const result = await operation();
      if (!disposed && version === revision) {
        if (result) {
          setStatuses((items) => [
            ...items.filter((item) => item.provider !== result.provider),
            visibleStatus(result),
          ]);
          nextPoll = Date.now() + Math.max(1000, result.pollAfterMs ?? 5000);
        }
        onActionComplete();
      }
    } catch {
      if (!disposed && version === revision) setError(failureMessage);
    } finally {
      if (!disposed && version === revision) setBusy(false);
    }
  };
  onMount(() => {
    void refresh();
    const timer = window.setInterval(() => {
      if (!desktop || busy()) return;
      if (selected() === 'github' && current().status === 'pending' && Date.now() >= nextPoll)
        void perform(() => port.advanceConnection('github'));
      else void refresh();
    }, 1500);
    onCleanup(() => {
      disposed = true;
      revision++;
      clearQr();
      window.clearInterval(timer);
    });
  });

  const selectProvider = (provider: SourceProvider) => {
    ++revision;
    clearQr();
    setSelected(provider);
    setError('');
    setPollError('');
  };
  const activity: ConnectionActivity = {
    busy,
    start: startAction,
    active: () => !disposed,
    fail: (message) => {
      if (!disposed) setError(message);
    },
    finish: () => {
      if (!disposed) setBusy(false);
    },
  };
  return {
    desktop,
    selected,
    busy,
    error,
    current,
    qrCode,
    selectProvider,
    activity,
    configure: (request: ConnectionConfig) => perform(() => port.configureConnection(request)),
    begin: (method?: TelegramAuthMethod) => perform(() => port.beginConnection(selected(), method)),
    advance: (input?: string) => perform(() => port.advanceConnection(selected(), input)),
    disconnect: () => perform(() => port.disconnectConnection(selected())),
    openPage: (page: ConnectionPage) =>
      perform(
        () => port.openConnectionPage(page),
        page === 'vk-auth'
          ? 'Не удалось открыть страницу входа. Если попытка истекла, отмените подключение и войдите заново.'
          : 'Не удалось открыть экспорт Instagram. Откройте Центр аккаунтов в настройках Instagram.',
      ),
  };
}
