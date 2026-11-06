import type { SourceProvider } from './sources';

export const connectionProviders = ['telegram', 'vk', 'github', 'instagram'] as const;

export type ConnectionState =
  | 'unconfigured'
  | 'disconnected'
  | 'pending'
  | 'phone-required'
  | 'code-required'
  | 'password-required'
  | 'email-required'
  | 'email-code-required'
  | 'connected'
  | 'error'
  | 'unavailable';

/** Native status contains public configuration and sanitized display text, never credentials. */
export interface ConnectionStatus {
  readonly provider: SourceProvider;
  readonly status: ConnectionState;
  readonly label?: string;
  readonly message?: string;
  readonly 'client-id'?: string;
  readonly 'api-id'?: number;
  readonly 'user-code'?: string;
  readonly 'poll-after-ms'?: number;
  readonly capabilities: readonly string[];
}

/** One-use input for native IPC. Never persist, log, return or pass to a model. */
export interface ConnectionConfig {
  readonly provider: SourceProvider;
  readonly 'client-id'?: string;
  readonly 'api-id'?: number;
  readonly 'api-hash'?: string;
  readonly token?: string;
}

export interface ImportedSource {
  readonly id: string;
  readonly label: string;
  readonly 'document-count': number;
  readonly warnings: readonly string[];
}
