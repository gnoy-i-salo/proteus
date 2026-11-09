// Strict validation primitives for the schemaVersion 4 saved-result reader.
// Every guard reports the dotted path of the offending field and never mutates input.

export const maxEnvelopeBytes = 8 * 1024 * 1024;
// Any nonempty JSON array needs at least two bytes per entry (value plus
// separator). The envelope bound covers collections with no writer-owned cap.
const defaultMaxItems = maxEnvelopeBytes / 2;

export function invalid(path: string, detail: string): never {
  throw new Error(`Invalid stored v4 result at ${path}: ${detail}.`);
}

export function asRecord(value: unknown, path: string): Record<string, unknown> {
  if (!value || typeof value !== 'object' || Array.isArray(value))
    invalid(path, 'expected an object');
  return value as Record<string, unknown>;
}

/** Rejects unknown or legacy-alias keys on schema-owned objects. */
export function exactKeys(
  record: Record<string, unknown>,
  allowed: readonly string[],
  path: string,
): void {
  for (const key of Object.keys(record)) {
    if (!allowed.includes(key)) invalid(path, `unknown field "${key}"`);
  }
}

export function field(record: Record<string, unknown>, key: string, path: string): unknown {
  if (!Object.hasOwn(record, key)) invalid(path, `missing field "${key}"`);
  return record[key];
}

export function optionalField(record: Record<string, unknown>, key: string): unknown {
  return Object.hasOwn(record, key) ? record[key] : undefined;
}

export function asText(value: unknown, path: string): string {
  if (typeof value !== 'string') invalid(path, 'expected a string');
  return value;
}

export function asNullableText(value: unknown, path: string): string | null {
  return value === null ? null : asText(value, path);
}

export function asBoolean(value: unknown, path: string): boolean {
  if (typeof value !== 'boolean') invalid(path, 'expected a boolean');
  return value;
}

export function asChoice<T extends string>(value: unknown, allowed: readonly T[], path: string): T {
  if (typeof value !== 'string' || !allowed.includes(value as T))
    invalid(path, `expected one of ${allowed.map((item) => `"${item}"`).join(', ')}`);
  return value as T;
}

export function asNumber(value: unknown, path: string, min?: number, max?: number): number {
  if (typeof value !== 'number' || !Number.isFinite(value))
    invalid(path, 'expected a finite number');
  if (min !== undefined && value < min) invalid(path, `expected a number >= ${min}`);
  if (max !== undefined && value > max) invalid(path, `expected a number <= ${max}`);
  return value;
}

export function asInteger(value: unknown, path: string, min?: number, max?: number): number {
  const number = asNumber(value, path, min, max);
  if (!Number.isSafeInteger(number)) invalid(path, 'expected a safe integer');
  return number;
}

const timestampPattern = /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z$/u;

export function asTimestamp(value: unknown, path: string): string {
  const text = asText(value, path);
  if (
    !timestampPattern.test(text) ||
    !Number.isFinite(Date.parse(text)) ||
    new Date(text).toISOString() !== text
  )
    invalid(path, 'expected an ISO-8601 timestamp (YYYY-MM-DDTHH:MM:SS.sssZ)');
  return text;
}

export function asNullableTimestamp(value: unknown, path: string): string | null {
  return value === null ? null : asTimestamp(value, path);
}

export function asArray(
  value: unknown,
  path: string,
  maxItems: number = defaultMaxItems,
): unknown[] {
  if (!Array.isArray(value) || value.length > maxItems)
    invalid(path, `expected an array of at most ${maxItems} items`);
  return value;
}

export function asStringArray(value: unknown, path: string, maxItems?: number): string[] {
  return asArray(value, path, maxItems).map((item, index) => asText(item, `${path}[${index}]`));
}

export interface IntervalScale {
  readonly min: number;
  readonly max: number;
}

export function asInterval(
  value: unknown,
  path: string,
  scale: IntervalScale,
): { lower: number; upper: number } {
  const record = asRecord(value, path);
  exactKeys(record, ['lower', 'upper'], path);
  const lower = asNumber(field(record, 'lower', path), `${path}.lower`, scale.min, scale.max);
  const upper = asNumber(field(record, 'upper', path), `${path}.upper`, lower, scale.max);
  return { lower, upper };
}

export function asNullableInterval(
  value: unknown,
  path: string,
  scale: IntervalScale,
): { lower: number; upper: number } | null {
  return value === null ? null : asInterval(value, path, scale);
}

/** Decodes an opaque map with validated values, preserving exact keys (including "__proto__") as inert data. */
export function asMap<T>(
  value: unknown,
  path: string,
  decodeValue: (item: unknown, itemPath: string) => T,
  maxItems: number = defaultMaxItems,
): Record<string, T> {
  const record = asRecord(value, path);
  const keys = Object.keys(record);
  if (keys.length > maxItems) invalid(path, `expected at most ${maxItems} entries`);
  return Object.fromEntries(
    keys.map((key) => [key, decodeValue(record[key], `${path}[${JSON.stringify(key)}]`)]),
  );
}

/** Decodes an opaque string map, preserving exact keys (including "__proto__") as inert data. */
export function asStringMap(
  value: unknown,
  path: string,
  maxItems: number = defaultMaxItems,
): Record<string, string> {
  const record = asRecord(value, path);
  const keys = Object.keys(record);
  if (keys.length > maxItems) invalid(path, `expected at most ${maxItems} entries`);
  return Object.fromEntries(
    keys.map((key) => [key, asText(record[key], `${path}[${JSON.stringify(key)}]`)]),
  );
}

/** Rejects envelopes above the native 8 MiB save bound (UTF-8 bytes of the serialized JSON). */
export function checkEnvelopeSize(value: unknown): void {
  let serialized: string | undefined;
  try {
    serialized = JSON.stringify(value);
  } catch {
    invalid('$', 'expected a JSON-serializable value');
  }
  if (serialized === undefined) invalid('$', 'expected a JSON-serializable value');
  if (new TextEncoder().encode(serialized).length > maxEnvelopeBytes)
    invalid('$', 'envelope exceeds the 8 MiB native save bound');
}
