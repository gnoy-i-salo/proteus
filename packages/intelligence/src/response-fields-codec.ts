/** A fixed, schema-owned pair. Callers supply literal pairs, never inferred names. */
export type ResponseFieldPair = readonly [legacy: string, canonical: string];

/**
 * Decode one object's known field names. This boundary is intentionally flat:
 * nested values, opaque maps and text remain untouched. Each owning parser
 * supplies its exact schema pairs and still validates the returned field values.
 */
export function decodeResponseFields(
  value: unknown,
  fields: readonly ResponseFieldPair[],
): Record<string, unknown> {
  if (!value || typeof value !== 'object' || Array.isArray(value))
    throw new Error('Expected a model response object');
  const names = new Set<string>();
  const aliases = new Map<string, string>();
  let hasLegacy = false;
  let hasCanonical = false;
  for (const [legacy, canonical] of fields) {
    if (!legacy || !canonical || legacy === canonical || names.has(legacy) || names.has(canonical))
      throw new Error('Ambiguous model response field schema');
    names.add(legacy);
    names.add(canonical);
    aliases.set(legacy, canonical);
    hasLegacy ||= Object.hasOwn(value, legacy);
    hasCanonical ||= Object.hasOwn(value, canonical);
  }
  if (hasLegacy && hasCanonical) throw new Error('Mixed legacy and canonical response fields');
  return Object.fromEntries(
    Object.entries(value).map(([key, item]) => [aliases.get(key) ?? key, item]),
  );
}
