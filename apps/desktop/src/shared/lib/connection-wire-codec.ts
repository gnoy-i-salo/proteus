import type { ConnectionState, SourceProvider } from '@proteus-int/contracts';

// Public status and transient configuration are separate; secrets are never status fields.
export interface NativeConnectionStatus {
  readonly provider: SourceProvider;
  readonly status: ConnectionState;
  readonly label?: string;
  readonly message?: string;
  readonly clientId?: string;
  readonly apiId?: number;
  readonly userCode?: string;
  readonly qrCode?: string;
  readonly pollAfterMs?: number;
  readonly capabilities: readonly string[];
}
/** One-use native input only. Never persist, export, log or pass to a model. */
export interface NativeConnectionConfig {
  readonly provider?: SourceProvider | null;
  readonly clientId?: string | null;
  readonly apiId?: number | null;
  readonly apiHash?: string | null;
  readonly token?: string | null;
}
export interface NativeImportedSource {
  readonly id: string;
  readonly label: string;
  readonly documentCount: number;
  readonly warnings: readonly string[];
}
function fields(value: unknown, allowed: readonly string[]): Record<string, unknown> {
  if (
    !value ||
    typeof value !== 'object' ||
    Array.isArray(value) ||
    Object.keys(value).some((key) => !allowed.includes(key))
  )
    throw new Error('Неверная схема подключения.');
  return value as Record<string, unknown>;
}
function text(value: unknown): string {
  if (typeof value !== 'string') throw new Error('Неверное текстовое поле подключения.');
  return value;
}
function nullableText(value: unknown): string | null {
  return value === null ? null : text(value);
}
function integer(value: unknown, minimum = 0, maximum = Number.MAX_SAFE_INTEGER): number {
  if (
    typeof value !== 'number' ||
    !Number.isSafeInteger(value) ||
    value < minimum ||
    value > maximum
  )
    throw new Error('Неверное числовое поле подключения.');
  return value;
}
function apiId(value: unknown): number {
  return integer(value, -0x80000000, 0x7fffffff);
}
function strings(value: unknown): string[] {
  if (!Array.isArray(value)) throw new Error('Неверный список подключения.');
  return value.map(text);
}
function choice<T extends string>(value: unknown, choices: readonly T[]): T {
  const selected = choices.find((item) => item === value);
  if (selected === undefined) throw new Error('Неизвестное значение поля подключения.');
  return selected;
}
function provider(value: unknown): SourceProvider {
  return choice(value, ['telegram', 'vk', 'instagram', 'github'] as const);
}
function qrCode(value: unknown): string {
  const code = text(value);
  if (new TextEncoder().encode(code).length > 128 * 1024)
    throw new Error('QR подключения превышает 128 КиБ.');
  return code;
}

export function decodeLegacyConnectionStatus(value: unknown): NativeConnectionStatus {
  const source = fields(value, [
    'provider',
    'status',
    'label',
    'message',
    'client-id',
    'api-id',
    'user-code',
    'qr-code',
    'poll-after-ms',
    'capabilities',
  ]);
  return {
    provider: provider(source.provider),
    status: choice(source.status, [
      'unconfigured',
      'disconnected',
      'pending',
      'qr-required',
      'phone-required',
      'code-required',
      'password-required',
      'email-required',
      'email-code-required',
      'connected',
      'error',
      'unavailable',
    ] as const),
    ...(Object.hasOwn(source, 'label') ? { label: text(source.label) } : {}),
    ...(Object.hasOwn(source, 'message') ? { message: text(source.message) } : {}),
    ...(Object.hasOwn(source, 'client-id') ? { clientId: text(source['client-id']) } : {}),
    ...(Object.hasOwn(source, 'api-id') ? { apiId: apiId(source['api-id']) } : {}),
    ...(Object.hasOwn(source, 'user-code') ? { userCode: text(source['user-code']) } : {}),
    ...(Object.hasOwn(source, 'qr-code') ? { qrCode: qrCode(source['qr-code']) } : {}),
    ...(Object.hasOwn(source, 'poll-after-ms')
      ? { pollAfterMs: integer(source['poll-after-ms']) }
      : {}),
    capabilities: strings(source.capabilities),
  };
}

function config(value: unknown, legacy: boolean): NativeConnectionConfig {
  const source = fields(value, [
    'provider',
    'token',
    ...(legacy ? ['client-id', 'api-id', 'api-hash'] : ['clientId', 'apiId', 'apiHash']),
  ]);
  const clientKey = legacy ? 'client-id' : 'clientId';
  const apiIdKey = legacy ? 'api-id' : 'apiId';
  const hashKey = legacy ? 'api-hash' : 'apiHash';
  return {
    ...(Object.hasOwn(source, 'provider')
      ? {
          provider: source.provider === null ? null : provider(source.provider),
        }
      : {}),
    ...(Object.hasOwn(source, clientKey) ? { clientId: nullableText(source[clientKey]) } : {}),
    ...(Object.hasOwn(source, apiIdKey)
      ? { apiId: source[apiIdKey] === null ? null : apiId(source[apiIdKey]) }
      : {}),
    ...(Object.hasOwn(source, hashKey) ? { apiHash: nullableText(source[hashKey]) } : {}),
    ...(Object.hasOwn(source, 'token') ? { token: nullableText(source.token) } : {}),
  };
}
export function decodeLegacyConnectionConfig(value: unknown): NativeConnectionConfig {
  return config(value, true);
}
export function encodeLegacyConnectionConfig(value: unknown) {
  const source = config(value, false);
  return {
    ...(Object.hasOwn(source, 'provider') ? { provider: source.provider } : {}),
    ...(Object.hasOwn(source, 'clientId') ? { 'client-id': source.clientId } : {}),
    ...(Object.hasOwn(source, 'apiId') ? { 'api-id': source.apiId } : {}),
    ...(Object.hasOwn(source, 'apiHash') ? { 'api-hash': source.apiHash } : {}),
    ...(Object.hasOwn(source, 'token') ? { token: source.token } : {}),
  };
}
export function decodeLegacyImportedSource(value: unknown): NativeImportedSource | null {
  if (value === null) return null;
  const source = fields(value, ['id', 'label', 'document-count', 'warnings']);
  return {
    id: text(source.id),
    label: text(source.label),
    documentCount: integer(source['document-count']),
    warnings: strings(source.warnings),
  };
}
