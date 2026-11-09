import type { ModelContextRequest } from './memory';
import { fields } from './corpus-wire-validation';
import { memoryText as text, memoryId as id, memoryIds as ids } from './memory-wire-validation';

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
