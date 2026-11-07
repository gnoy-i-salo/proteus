import type { CollectionAction } from '@proteus-int/contracts';
import { decodeResponseFields } from '@proteus-int/intelligence';

export function parseCollectionAction(raw: Record<string, unknown>): CollectionAction {
  const value = decodeResponseFields(raw, [['document-ids', 'documentIds']]);
  const allowed: Record<string, readonly string[]> = {
    describe: ['action'],
    coverage: ['action'],
    search: ['action', 'query', 'limit'],
    collect: ['action', 'cursor', 'limit'],
    read: ['action', 'documentIds'],
    finish: ['action', 'reason'],
  };
  const fields = typeof value.action === 'string' ? allowed[value.action] : undefined;
  const fail = () => {
    throw new Error('STALKER: invalid source action');
  };
  if (!fields || Object.keys(value).some((key) => !fields.includes(key))) return fail();
  const bounded = (item: unknown, max: number): item is string =>
    typeof item === 'string' && item.trim().length > 0 && item.length <= max;
  if (
    (value.action === 'collect' || value.action === 'search') &&
    (!Number.isInteger(value.limit) || Number(value.limit) < 1 || Number(value.limit) > 20)
  )
    return fail();
  if (value.action === 'search' && !bounded(value.query, 256)) return fail();
  if (value.action === 'collect' && value.cursor !== null && !bounded(value.cursor, 1024))
    return fail();
  if (
    value.action === 'read' &&
    (!Array.isArray(value.documentIds) ||
      !value.documentIds.length ||
      value.documentIds.length > 20 ||
      value.documentIds.some((id) => !bounded(id, 128)))
  )
    return fail();
  if (value.action === 'finish' && !bounded(value.reason, 1000)) return fail();
  return value as unknown as CollectionAction;
}
