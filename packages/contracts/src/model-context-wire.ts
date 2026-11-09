import type { ModelCallBudget, ModelContextLease, ModelContextRequest } from './memory';
import type { ResearchSessionDescriptor } from './research-memory';
import { fields, integer, list } from './corpus-wire-validation';
import { decodeModelCallBudget } from './research-memory-codec';
import { decodeSourceExcerpt } from './source-excerpt-codec';
import { memoryId, memoryIds, memoryText } from './memory-wire-validation';

// Shared structural input/usage reader. Native still owns exact hashes,
// one-shot dispatch, model binding and current permission checks.
function digest(value: unknown): string {
  if (typeof value !== 'string' || !/^[a-f0-9]{64}$/u.test(value))
    throw new Error('Неверный отпечаток контекста.');
  return value;
}

export function decodeContextWire(
  value: unknown,
  session: ResearchSessionDescriptor,
  expected: Pick<ModelContextRequest, 'systemPrompt' | 'task' | 'formatAttempt'>,
  modelBudget: ModelCallBudget,
  purpose: 'research' | 'fabric' | 'personaReply',
  manifestId: string | null = null,
): { base: Omit<ModelContextLease, 'purpose' | 'items'>; items: unknown[] } {
  const budget = decodeModelCallBudget(modelBudget);
  const input = fields(value, [
    'schemaVersion',
    'id',
    'singleUse',
    'personaId',
    'inputSha256',
    'inputBytes',
    'systemPrompt',
    'task',
    'corpusSnapshotId',
    'manifestId',
    'purpose',
    'formatAttempt',
    'policyVersion',
    'receiptIds',
    'items',
    'sources',
    'contextSha256',
    'expiresAt',
    'usage',
  ]);
  if (
    input.schemaVersion !== 2 ||
    input.singleUse !== true ||
    input.personaId !== session.personaId ||
    input.corpusSnapshotId !== session.corpusSnapshotId ||
    input.policyVersion !== session.policyVersion ||
    input.purpose !== purpose ||
    input.manifestId !== manifestId ||
    (purpose === 'personaReply') !== (manifestId !== null) ||
    input.formatAttempt !== expected.formatAttempt ||
    input.expiresAt !== session.expiresAt
  )
    throw new Error('Контекст не соответствует исследовательской сессии.');
  const receiptIds = memoryIds(input.receiptIds);
  const systemPrompt = memoryText(input.systemPrompt, 40000);
  const task = memoryText(input.task, 200000);
  const inputBytes = integer(input.inputBytes, 1);
  if (
    systemPrompt !== expected.systemPrompt ||
    !task.startsWith(expected.task) ||
    task.length === expected.task.length ||
    inputBytes !== new TextEncoder().encode(JSON.stringify([systemPrompt, task])).length
  )
    throw new Error('Неверный вход контекста модели.');
  const rawSources = list(input.sources);
  const sources = rawSources.map(decodeSourceExcerpt);
  const rawUsage = fields(input.usage, ['modelCalls', 'inputBytes']);
  const usage = {
    modelCalls: integer(rawUsage.modelCalls, 1),
    inputBytes: integer(rawUsage.inputBytes, inputBytes),
  };
  if (usage.modelCalls > budget.maxCalls || usage.inputBytes > budget.maxInputBytes)
    throw new Error('Контекст превышает бюджет модели.');
  return {
    items: list(input.items),
    base: {
      schemaVersion: 2,
      id: memoryId(input.id),
      singleUse: true,
      personaId: session.personaId,
      inputSha256: digest(input.inputSha256),
      inputBytes,
      systemPrompt,
      task,
      corpusSnapshotId: session.corpusSnapshotId,
      manifestId,
      formatAttempt: expected.formatAttempt,
      policyVersion: session.policyVersion,
      receiptIds,
      sources,
      contextSha256: digest(input.contextSha256),
      expiresAt: session.expiresAt,
      usage,
    },
  };
}
