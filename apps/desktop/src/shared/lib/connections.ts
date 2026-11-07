import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type {
  ConnectionConfig,
  ConnectionPage,
  ConnectionStatus,
  TelegramAuthMethod,
  ImportedSource,
  SourceProvider,
  SourceGrant,
  SourceGrantRequest,
  SourcePort,
  SourceResult,
} from '@proteus-int/contracts';
import { isDesktop } from './window-controls';
import {
  decodeLegacyConnectionStatus,
  decodeLegacyImportedSource,
  encodeLegacyConnectionConfig,
} from './connection-wire-codec';
import {
  decodeLegacySourceGrant,
  decodeLegacySourceResult,
  encodeLegacySourceAction,
  encodeLegacySourceGrantRequest,
} from './source-wire-codec';

function requireDesktop(): void {
  if (!isDesktop()) throw new Error('Подключения доступны в настольном приложении PROTEUS.');
}

export function listenConnectionErrors(onError: () => void): Promise<() => void> {
  requireDesktop();
  return listen('connection-error', () => onError());
}

export function connectionStatuses(): Promise<ConnectionStatus[]> {
  requireDesktop();
  return invoke<unknown>('connection_statuses').then((value) => {
    if (!Array.isArray(value)) throw new Error('Неверный список подключений.');
    return value.map(decodeLegacyConnectionStatus);
  });
}

export function configureConnection(request: ConnectionConfig): Promise<ConnectionStatus> {
  requireDesktop();
  return invoke('configure_connection', { request: encodeLegacyConnectionConfig(request) }).then(
    decodeLegacyConnectionStatus,
  );
}

export function beginConnection(
  provider: SourceProvider,
  telegramMethod?: TelegramAuthMethod,
): Promise<ConnectionStatus> {
  requireDesktop();
  return invoke('begin_connection', {
    provider,
    ...(telegramMethod ? { telegramMethod } : {}),
  }).then(decodeLegacyConnectionStatus);
}

export function openConnectionPage(page: ConnectionPage): Promise<void> {
  requireDesktop();
  return invoke('open_connection_page', { page });
}

export function advanceConnection(
  provider: SourceProvider,
  input?: string,
): Promise<ConnectionStatus> {
  requireDesktop();
  return invoke('advance_connection', { provider, ...(input === undefined ? {} : { input }) }).then(
    decodeLegacyConnectionStatus,
  );
}

export function disconnectConnection(provider: SourceProvider): Promise<ConnectionStatus> {
  requireDesktop();
  return invoke('disconnect_connection', { provider }).then(decodeLegacyConnectionStatus);
}

export function importSource(provider: SourceProvider): Promise<ImportedSource | null> {
  requireDesktop();
  return invoke('import_source', { provider }).then(decodeLegacyImportedSource);
}

export function createSourceGrant(request: SourceGrantRequest): Promise<SourceGrant> {
  requireDesktop();
  return invoke('create_source_grant', { request: encodeLegacySourceGrantRequest(request) }).then(
    decodeLegacySourceGrant,
  );
}

export function cancelSourceGrant(grantId: string): Promise<void> {
  requireDesktop();
  return invoke('cancel_source', { grantId });
}

export function deleteSourceGrant(grantId: string): Promise<void> {
  requireDesktop();
  return invoke('delete_source', { grantId });
}

export function resumeSourceGrant(grantId: string): Promise<void> {
  requireDesktop();
  return invoke('resume_source', { grantId });
}

export function clearSourceData(provider: SourceProvider): Promise<void> {
  requireDesktop();
  return invoke('clear_source_data', { provider });
}

export const nativeSourcePort: SourcePort = {
  async reserveModelCall(grantId, signal) {
    requireDesktop();
    signal?.throwIfAborted();
    await invoke('reserve_model_step', { grantId });
    signal?.throwIfAborted();
  },
  async execute(grantId, action, signal) {
    requireDesktop();
    signal?.throwIfAborted();
    const cancel = () => {
      void cancelSourceGrant(grantId).catch(() => undefined);
    };
    signal?.addEventListener('abort', cancel, { once: true });
    try {
      const result: SourceResult = decodeLegacySourceResult(
        await invoke('source_action', { grantId, action: encodeLegacySourceAction(action) }),
      );
      signal?.throwIfAborted();
      return result;
    } finally {
      signal?.removeEventListener('abort', cancel);
    }
  },
};
