export type JsonObject = Record<string, unknown>;
export function object(value: unknown): JsonObject {
  if (!value || typeof value !== 'object' || Array.isArray(value))
    throw new Error('Expected a JSON object');
  return value as JsonObject;
}
export function text(value: unknown, field = 'text'): string {
  if (typeof value !== 'string' || !value.trim() || value.length > 24000)
    throw new Error(`Invalid JSON field: ${field}`);
  return value.trim();
}
export function texts(value: unknown, field = 'list'): string[] {
  if (!Array.isArray(value) || value.length > 30) throw new Error(`Invalid JSON field: ${field}`);
  return value.map((item) => text(item, field));
}
