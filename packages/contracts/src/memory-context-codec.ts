import type { ModelContextRequest } from './memory';
import { fields, list } from './corpus-wire-validation';

const encoder = new TextEncoder();
function text(value: unknown, maximumBytes: number): string {
  if (typeof value !== 'string' || encoder.encode(value).length > maximumBytes)
    throw new Error('Превышен размер входа модели.');
  return value;
}
function id(value: unknown): string {
  const result = text(value, 256);
  if (!result.trim() || /\p{Cc}/u.test(result)) throw new Error('Неверный идентификатор памяти.');
  return result;
}
function ids(value: unknown): string[] {
  const values = list(value);
  if (values.length > 1000) throw new Error('Слишком много ссылок контекста.');
  const result = values.map(id);
  if (new Set(result).size !== result.length) throw new Error('Повторяющиеся ссылки контекста.');
  return result;
}

/** V2 supplies base input; native assembles sources and computes dispatch identity. */
export function decodeModelContextRequest(value: unknown): ModelContextRequest {
  try {
    const input = fields(value, [
      'schemaVersion',
      'purpose',
      'manifestId',
      'systemPrompt',
      'task',
      'receiptIds',
      'memoryItemIds',
      'formatAttempt',
    ]);
    if (input.schemaVersion !== 2 || (input.formatAttempt !== 1 && input.formatAttempt !== 2))
      throw new Error('Неизвестная версия или попытка форматирования.');
    const common = {
      schemaVersion: 2 as const,
      systemPrompt: text(input.systemPrompt, 40000),
      task: text(input.task, 200000),
      receiptIds: ids(input.receiptIds),
      memoryItemIds: ids(input.memoryItemIds),
      formatAttempt: input.formatAttempt,
    } satisfies Omit<ModelContextRequest, 'purpose' | 'manifestId'>;
    if (input.purpose === 'research' && input.manifestId === null && !common.memoryItemIds.length)
      return { ...common, purpose: 'research', manifestId: null };
    if (input.purpose === 'personaReply')
      return { ...common, purpose: 'personaReply', manifestId: id(input.manifestId) };
    throw new Error('Назначение контекста не соответствует снимку памяти.');
  } catch (cause) {
    throw new Error('Неверный запрос контекста модели.', { cause });
  }
}
