import type { ModelCallBudget, ModelContextLease, ModelContextRequest } from './memory';
import type { ResearchSessionDescriptor } from './research-memory';
import type { SourceExcerpt } from './corpus';
import { fields, integer, list } from './corpus-wire-validation';
import { decodeModelContextRequest } from './memory-context-codec';
import { decodeModelCallBudget } from './research-memory-codec';
import { decodeSourceExcerpt } from './source-excerpt-codec';
import { memoryId, memoryIds, memoryText } from './memory-wire-validation';

function digest(value: unknown): string {
  if (typeof value !== 'string' || !/^[a-f0-9]{64}$/u.test(value))
    throw new Error('Неверный отпечаток контекста.');
  return value;
}

/** Structural IPC reader. Native revalidates hashes, one-shot use and live access at dispatch. */

export function decodeResearchContext(
  value: unknown,
  session: ResearchSessionDescriptor,
  request: ModelContextRequest,
  modelBudget: ModelCallBudget,
  expectedSources: readonly SourceExcerpt[],
): ModelContextLease {
  const expected = decodeModelContextRequest(request);
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
    expected.purpose !== 'research' ||
    input.schemaVersion !== 2 ||
    input.singleUse !== true ||
    input.personaId !== session.personaId ||
    input.corpusSnapshotId !== session.corpusSnapshotId ||
    input.policyVersion !== session.policyVersion ||
    input.purpose !== 'research' ||
    input.manifestId !== null ||
    input.formatAttempt !== expected.formatAttempt ||
    input.expiresAt !== session.expiresAt ||
    list(input.items).length !== 0
  )
    throw new Error('Контекст не соответствует исследовательской сессии.');
  const receiptIds = memoryIds(input.receiptIds);
  if (JSON.stringify(receiptIds) !== JSON.stringify(expected.receiptIds))
    throw new Error('Контекст не соответствует выбранным квитанциям.');
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
  if (rawSources.length !== expectedSources.length)
    throw new Error('Контекст не соответствует выбранным источникам.');
  const sources = rawSources.map(decodeSourceExcerpt);
  if (
    sources.some(
      (source, index) =>
        JSON.stringify(source) !== JSON.stringify(decodeSourceExcerpt(expectedSources[index])),
    )
  )
    throw new Error('Контекст изменяет выбранные источники.');
  const rawUsage = fields(input.usage, ['modelCalls', 'inputBytes']);
  const usage = {
    modelCalls: integer(rawUsage.modelCalls, 1),
    inputBytes: integer(rawUsage.inputBytes, inputBytes),
  };
  if (usage.modelCalls > budget.maxCalls || usage.inputBytes > budget.maxInputBytes)
    throw new Error('Контекст превышает бюджет модели.');
  return {
    schemaVersion: 2,
    id: memoryId(input.id),
    singleUse: true,
    personaId: session.personaId,
    inputSha256: digest(input.inputSha256),
    inputBytes,
    systemPrompt,
    task,
    corpusSnapshotId: session.corpusSnapshotId,
    manifestId: null,
    purpose: 'research',
    formatAttempt: expected.formatAttempt,
    policyVersion: session.policyVersion,
    receiptIds,
    items: [],
    sources,
    contextSha256: digest(input.contextSha256),
    expiresAt: session.expiresAt,
    usage,
  };
}
