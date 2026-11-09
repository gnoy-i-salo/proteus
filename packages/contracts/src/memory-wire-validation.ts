import { list } from './corpus-wire-validation';

const encoder = new TextEncoder();
const decoder = new TextDecoder('utf-8', { fatal: true, ignoreBOM: true });
export function memoryText(value: unknown, maximumBytes: number): string {
  if (typeof value !== 'string') throw new Error('Неверный текст входа модели.');
  const bytes = encoder.encode(value);
  if (bytes.length > maximumBytes) throw new Error('Превышен размер входа модели.');
  if (decoder.decode(bytes) !== value) throw new Error('Вход модели содержит неверный Unicode.');
  return value;
}
export function memoryId(value: unknown, maximumBytes = 256): string {
  const result = memoryText(value, maximumBytes);
  if (!result.trim() || /\p{Cc}/u.test(result)) throw new Error('Неверный идентификатор памяти.');
  return result;
}
export function memoryIds(value: unknown): string[] {
  const values = list(value);
  if (values.length > 1000) throw new Error('Слишком много ссылок контекста.');
  const result = values.map((value) => memoryId(value));
  if (new Set(result).size !== result.length) throw new Error('Повторяющиеся ссылки контекста.');
  return result;
}

export function memoryTimestamp(value: unknown, message = 'Неверная дата памяти.'): string {
  const result = memoryText(value, 64);
  const time = Date.parse(result);
  if (!Number.isFinite(time) || new Date(time).toISOString() !== result) throw new Error(message);
  return result;
}

export function memoryFields(value: unknown, expected: readonly string[]): Record<string, unknown> {
  if (!value || typeof value !== 'object' || Array.isArray(value))
    throw new Error('Invalid memory receipt object.');
  const keys = Object.keys(value);
  if (keys.length !== expected.length || keys.some((key) => !expected.includes(key)))
    throw new Error('Unknown, missing or mixed memory receipt fields.');
  return value as Record<string, unknown>;
}

// Retain the legacy receipt/action character limit; do not reinterpret held data as UTF8 byte limits.
export function memoryReceiptText(value: unknown, allowEmpty = false): string {
  if (typeof value !== 'string' || (!allowEmpty && !value.trim()) || value.length > 2048)
    throw new Error('Invalid memory receipt text.');
  return value;
}
