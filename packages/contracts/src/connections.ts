import type { SourceProvider } from './sources';

export const connectionProviders = ['telegram', 'vk', 'github', 'instagram'] as const;
export type TelegramAuthMethod = 'qr' | 'phone';
export type ConnectionPage = 'vk-auth' | 'instagram-export';

export type ConnectionState =
  | 'unconfigured'
  | 'disconnected'
  | 'pending'
  | 'qr-required'
  | 'phone-required'
  | 'code-required'
  | 'password-required'
  | 'email-required'
  | 'email-code-required'
  | 'connected'
  | 'error'
  | 'unavailable';

/** Public configuration and ephemeral login challenges; never access tokens or PKCE verifiers.
 * Do not persist, log, export or send login challenges to a model.
 */
export interface ConnectionStatus {
  readonly provider: SourceProvider;
  readonly status: ConnectionState;
  readonly label?: string;
  readonly message?: string;
  readonly clientId?: string;
  readonly apiId?: number;
  readonly userCode?: string;
  /** Native-generated SVG image data URL, at most 128 KiB; only present while QR is required. */
  readonly qrCode?: string;
  readonly pollAfterMs?: number;
  readonly capabilities: readonly string[];
}

/** One-use input for native IPC. Never persist, log, return or pass to a model. */
export interface ConnectionConfig {
  readonly provider: SourceProvider;
  readonly clientId?: string;
  readonly apiId?: number;
  readonly apiHash?: string;
  readonly token?: string;
}

export interface ImportedSource {
  readonly id: string;
  readonly label: string;
  readonly documentCount: number;
  readonly warnings: readonly string[];
}
