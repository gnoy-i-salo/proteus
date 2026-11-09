import type { MemoryItem, RecallResult, RecallBudget } from './memory';

type Record = Pick<RecallResult, 'receipt' | 'sources'>;

export function verifyContextBudget(records: readonly Record[], budget: RecallBudget): void {
  const total = records.reduce(
    (sum, { receipt }) => ({
      candidates: sum.candidates + receipt.hits.length,
      episodes: sum.episodes + new Set(receipt.hits.map((hit) => hit.episodeId)).size,
      bytes: sum.bytes + receipt.sourceBytes,
      tokens: sum.tokens + receipt.contextTokens,
    }),
    { candidates: 0, episodes: 0, bytes: 0, tokens: 0 },
  );
  if (
    records.length > budget.maxCalls ||
    total.episodes > budget.maxEpisodes ||
    total.candidates > budget.maxCandidates ||
    total.bytes > budget.maxSourceBytes ||
    total.tokens > budget.maxContextTokens
  )
    throw new Error('Контекст превышает бюджет чтения памяти.');
}

export function verifyMemorySupport(
  items: readonly MemoryItem[],
  records: readonly Record[],
): void {
  const decoder = new TextDecoder('utf-8', { fatal: true, ignoreBOM: true });
  const sources = records.flatMap((record) =>
    record.sources.map((source, index) => ({
      source,
      episodeId: record.receipt.hits[index]!.episodeId,
      bytes: new TextEncoder().encode(source.text),
    })),
  );
  for (const item of items) {
    if (item.kind === 'reportedAssertion' && item.support === 'observed')
      throw new Error('Сообщение автора не является наблюдённым фактом.');
    const own = item.kind === 'styleExample' || item.support === 'observed';
    let exact = false;
    const episodes = new Set<string>();
    for (const span of item.sourceSpans) {
      const matches = sources.filter(
        ({ source }) =>
          source.span.documentId === span.documentId &&
          source.span.revisionId === span.revisionId &&
          source.span.start <= span.start &&
          source.span.end >= span.end,
      );
      if (!matches.length) throw new Error('Нет источника для элемента памяти.');
      let previous: string | undefined;
      for (const { source, bytes, episodeId } of matches) {
        const text = decoder.decode(
          bytes.subarray(span.start - source.span.start, span.end - source.span.start),
        );
        const identity = JSON.stringify([episodeId, text]);
        if (
          (own && !source.canSupportOwnWords) ||
          (previous !== undefined && previous !== identity)
        )
          throw new Error('Неоднозначное основание элемента памяти.');
        previous = identity;
        exact ||= text === item.text;
        episodes.add(episodeId);
      }
    }
    if ((own && !exact) || JSON.stringify([...episodes]) !== JSON.stringify(item.episodeIds))
      throw new Error('Основания не соответствуют элементу памяти.');
  }
}
