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
