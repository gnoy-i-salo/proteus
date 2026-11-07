import type { SourceBinding } from './corpus';

function fields(value: unknown, keys: readonly string[]): Record<string, unknown> {
  if (
    !value ||
    typeof value !== 'object' ||
    Array.isArray(value) ||
    Object.keys(value).length !== keys.length ||
    Object.keys(value).some((key) => !keys.includes(key))
  )
    throw new Error('Неверная схема привязки источника.');
  return value as Record<string, unknown>;
}
function text(value: unknown): string {
  if (typeof value !== 'string') throw new Error('Неверное текстовое поле источника.');
  return value;
}
function nullableText(value: unknown): string | null {
  return value === null ? null : text(value);
}
function flag(value: unknown): boolean {
  if (typeof value !== 'boolean') throw new Error('Неверный флаг привязки источника.');
  return value;
}
function choice<T extends string>(value: unknown, allowed: readonly T[]): T {
  const selected = allowed.find((item) => item === value);
  if (selected === undefined) throw new Error('Неизвестное поле привязки источника.');
  return selected;
}
function identifier(value: unknown): string {
  const id = text(value);
  if (!id.trim() || new TextEncoder().encode(id).length > 128 || /\p{Cc}/u.test(id))
    throw new Error('Неверный идентификатор привязки источника.');
  return id;
}
function contextIds(value: unknown, targetId: string | null): string[] {
  if (!Array.isArray(value) || value.length > 128) throw new Error('Выберите до 128 собеседников.');
  const ids = value.map(identifier);
  if (new Set(ids).size !== ids.length || (targetId !== null && ids.includes(targetId)))
    throw new Error('Идентификаторы авторов не должны повторяться.');
  return ids;
}
export function decodeSourceBinding(value: unknown): SourceBinding {
  const input = fields(value, [
    'schemaVersion',
    'id',
    'personaId',
    'provider',
    'accountId',
    'resourceId',
    'grantId',
    'policyVersion',
    'role',
    'targetAuthorId',
    'allowedContextAuthorIds',
    'allowUnknownContext',
    'period',
  ]);
  if (input.schemaVersion !== 1) throw new Error('Неизвестная версия привязки источника.');
  const selection = decodeBindingSelection(input);
  return {
    schemaVersion: 1,
    id: identifier(input.id),
    personaId: selection.personaId,
    provider: choice(input.provider, [
      'telegram',
      'vk',
      'instagram',
      'github',
      'web',
      'savedPage',
    ] as const),
    accountId: nullableText(input.accountId),
    resourceId: text(input.resourceId),
    grantId: identifier(input.grantId),
    policyVersion: text(input.policyVersion),
    role: selection.role,
    targetAuthorId: selection.targetAuthorId,
    allowedContextAuthorIds: selection.allowedContextAuthorIds,
    allowUnknownContext: selection.allowUnknownContext,
    period: selection.period,
  };
}

// Shared structural selection validation; the outer owning decoder checks its
// exact schema. This function never issues IDs, permissions or native authority.
export function decodeBindingSelection(
  input: Record<string, unknown>,
): Pick<
  SourceBinding,
  | 'personaId'
  | 'role'
  | 'targetAuthorId'
  | 'allowedContextAuthorIds'
  | 'allowUnknownContext'
  | 'period'
> {
  const period = fields(input.period, ['from', 'to', 'includeUndated']);
  const targetAuthorId = input.targetAuthorId === null ? null : identifier(input.targetAuthorId);
  const role = choice(input.role, ['own', 'context', 'about', 'reference'] as const);
  if (role === 'own' && targetAuthorId === null) throw new Error('В привязке отсутствует автор.');
  return {
    personaId: identifier(input.personaId),
    role,
    targetAuthorId,
    allowedContextAuthorIds: contextIds(input.allowedContextAuthorIds, targetAuthorId),
    allowUnknownContext: flag(input.allowUnknownContext),
    period: {
      from: nullableText(period.from),
      to: nullableText(period.to),
      includeUndated: flag(period.includeUndated),
    },
  };
}
