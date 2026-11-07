export function fields(value: unknown, keys: readonly string[]): Record<string, unknown> {
  if (
    !value ||
    typeof value !== 'object' ||
    Array.isArray(value) ||
    Object.keys(value).length !== keys.length ||
    Object.keys(value).some((key) => !keys.includes(key))
  )
    throw new Error('Неверная схема импорта.');
  return value as Record<string, unknown>;
}
export function text(value: unknown): string {
  if (typeof value !== 'string') throw new Error('Неверное текстовое поле импорта.');
  return value;
}
export function id(value: unknown): string {
  const result = text(value);
  if (!result.trim()) throw new Error('Отсутствует идентификатор импорта.');
  return result;
}
export function nullableText(value: unknown): string | null {
  return value === null ? null : text(value);
}
export function integer(value: unknown, minimum = 0): number {
  if (typeof value !== 'number' || !Number.isSafeInteger(value) || value < minimum)
    throw new Error('Неверный счётчик импорта.');
  return value;
}
export function flag(value: unknown): boolean {
  if (typeof value !== 'boolean') throw new Error('Неверный флаг импорта.');
  return value;
}
export function list(value: unknown): unknown[] {
  if (!Array.isArray(value)) throw new Error('Неверный список импорта.');
  return value as unknown[];
}
export function uniqueIds(value: unknown): string[] {
  const result = list(value).map(id);
  if (new Set(result).size !== result.length)
    throw new Error('Повторяющиеся идентификаторы импорта.');
  return result;
}
export function choice<T extends string>(value: unknown, allowed: readonly T[]): T {
  const result = allowed.find((item) => item === value);
  if (result === undefined) throw new Error('Неизвестное состояние импорта.');
  return result;
}
export function version(value: unknown): void {
  if (value !== 1) throw new Error('Неизвестная версия импорта.');
}
