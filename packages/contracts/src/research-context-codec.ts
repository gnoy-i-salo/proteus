import type { ModelCallBudget, ModelContextLease, ModelContextRequest } from './memory';
import type { ResearchSessionDescriptor } from './research-memory';
import type { SourceExcerpt } from './corpus';
import { decodeModelContextRequest } from './memory-context-codec';
import { decodeSourceExcerpt } from './source-excerpt-codec';
import { decodeContextWire } from './model-context-wire';

/** Structural IPC reader. Native revalidates hashes, one-shot use and live access at dispatch. */
export function decodeResearchContext(
  value: unknown,
  session: ResearchSessionDescriptor,
  request: ModelContextRequest,
  modelBudget: ModelCallBudget,
  expectedSources: readonly SourceExcerpt[],
): ModelContextLease {
  const expected = decodeModelContextRequest(request);
  const { base, items } = decodeContextWire(value, session, expected, modelBudget, 'research');
  if (expected.purpose !== 'research' || items.length !== 0)
    throw new Error('Контекст не соответствует исследовательской сессии.');
  if (JSON.stringify(base.receiptIds) !== JSON.stringify(expected.receiptIds))
    throw new Error('Контекст не соответствует выбранным квитанциям.');
  if (base.sources.length !== expectedSources.length)
    throw new Error('Контекст не соответствует выбранным источникам.');
  if (
    base.sources.some(
      (source, index) =>
        JSON.stringify(source) !== JSON.stringify(decodeSourceExcerpt(expectedSources[index])),
    )
  )
    throw new Error('Контекст изменяет выбранные источники.');
  return { ...base, purpose: 'research', items: [] };
}
