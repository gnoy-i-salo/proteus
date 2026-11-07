import { describe, expect, it, vi } from 'vitest';
import type { PersonInput, SourceDocument, SourcePort } from '@proteus-int/contracts';
import type { IntelligenceProvider } from '@proteus-int/intelligence';
import { analyzeSourceCollections as runAnalysis } from './source-analysis';
import { sourceDocument, sourceGrant, sourceInput, sourceResult } from './testing/source-fixtures';
import { sourceModelDocument } from './source-model-document';
import { collectSource, type SourceCollection } from './source-collection';

const analysisPort: SourcePort = {
  execute() {
    return Promise.reject(new Error('Analysis must not read sources'));
  },
  async reserveModelCall() {},
};
function analyzeSourceCollections(
  input: PersonInput,
  collections: readonly SourceCollection[],
  provider: IntelligenceProvider,
) {
  return runAnalysis(input, collections, provider, analysisPort);
}

function collection(documents = [sourceDocument()]): SourceCollection {
  return {
    grant: sourceGrant,
    documents,
    coverage: sourceResult(documents).coverage,
    traces: [],
    status: 'complete',
    limitations: [],
    usage: { steps: 2, bytes: 80, durationMs: 1 },
  };
}
function analyzer(change?: (value: Record<string, unknown>) => void) {
  const run = vi.fn<IntelligenceProvider['run']>().mockImplementation((request) => {
    const documents = (JSON.parse(request.task) as { documents: { id: string; text: string }[] })
      .documents;
    const value: Record<string, unknown> = {
      evidence: documents
        .filter((doc) => doc.id === 'd1')
        .map((doc) => ({
          id: 'e1',
          kind: 'fact',
          status: 'supported',
          summary: 'Проверяет примеры перед выводом',
          documentIds: [doc.id],
          excerpt: doc.text.slice(0, 60),
          retrievedAt: '2099-01-01T00:00:00Z',
          sourceUrl: 'https://invented.example/profile',
          limitations: [],
        })),
      character: 'unknown',
      thinkingStyle: 'Задаёт уточняющие вопросы',
      activity: 'unknown',
      behaviorSummary: 'unknown',
    };
    change?.(value);
    return Promise.resolve({
      answer: JSON.stringify(value),
      trace: { provider: 'codex', reasoningMs: 1, terminalMs: 0, summary: 'done' },
    });
  });
  return { id: 'codex' as const, run };
}
describe('source corpus analysis', () => {
  it.each([3, 40])(
    'keeps analysis and one format repair available after a long source walk with %i steps',
    async (maxSteps) => {
      const grant = { ...sourceGrant, budget: { ...sourceGrant.budget, maxSteps: maxSteps } };
      let spent = 0;
      let pages = 0;
      const reserve = () => {
        if (spent >= maxSteps) return Promise.reject(new Error('source-budget-exceeded'));
        spent++;

        return Promise.resolve();
      };
      const port: SourcePort = {
        async execute() {
          await reserve();
          const page = sourceResult([sourceDocument(`d${++pages}`)], `cursor-${pages}`);
          return { ...page, coverage: { ...page.coverage, documents: pages } };
        },
        reserveModelCall: vi.fn(reserve),
      };
      const provider = analyzer();
      provider.run.mockResolvedValueOnce({
        answer: 'invalid JSON',
        trace: { provider: 'codex', reasoningMs: 1, terminalMs: 0, summary: 'retry' },
      });
      const collected = await collectSource(grant, sourceInput, provider, port);
      const { report } = await runAnalysis(sourceInput, [collected], provider, port);
      expect(provider.run.mock.calls.length).toBeGreaterThanOrEqual(2);
      expect(provider.run.mock.calls[1]?.[0].formatAttempt).toBe(2);
      expect(
        provider.run.mock.calls.every(([request]) => request.operation === 'corpus-analysis'),
      ).toBe(true);
      expect(collected.status).toBe('partial');
      expect(collected.coverage.complete).toBe(false);
      expect(collected.usage.steps).toBe(pages);
      expect(collected.usage.steps).toBeLessThanOrEqual(maxSteps - 2);
      expect(collected.limitations.join(' ')).toMatch(/analysis.*budget|budget.*analysis/i);
      expect(report.sourceCoverage?.[0]).toMatchObject({ analyzedDocuments: pages });
      expect(spent).toBe(pages + provider.run.mock.calls.length);
      expect(spent).toBeLessThanOrEqual(maxSteps);
    },
  );
  it('allows a one-step grant to read once and reports that analysis could not run', async () => {
    const grant = { ...sourceGrant, budget: { ...sourceGrant.budget, maxSteps: 1 } };
    const execute = vi.fn().mockResolvedValue(sourceResult([sourceDocument()], 'next'));
    const reserveModelCall = vi.fn();
    const provider = analyzer();
    const port: SourcePort = { execute, reserveModelCall };
    const collected = await collectSource(grant, sourceInput, provider, port);
    const { report } = await runAnalysis(sourceInput, [collected], provider, port);
    expect(execute).toHaveBeenCalledOnce();
    expect(provider.run).not.toHaveBeenCalled();
    expect(reserveModelCall).not.toHaveBeenCalled();
    expect(collected.status).toBe('partial');
    expect(collected.limitations.join(' ')).toContain('no steps remain for analysis');
    expect(report.sourceCoverage?.[0]).toMatchObject({
      analyzedDocuments: 0,
      coverage: { notAnalyzed: 1 },
    });
    expect(report.sourceCoverage?.[0]?.limitations.join(' ')).toMatch(/analysis budget/i);
  });
  it('grounds facts in exact native text and never accepts model retrieval metadata', async () => {
    const provider = analyzer();
    const { report } = await analyzeSourceCollections(sourceInput, [collection()], provider);
    expect(report.evidence[0]).toMatchObject({
      verification: 'source-retrieved',
      retrievedAt: '2026-02-01T00:00:00Z',
      sourceUrl: 'https://example.org/ada',
      documentIds: ['d1'],
    });
    expect(provider.run.mock.calls[0]?.[0]).toHaveProperty('operation', 'corpus-analysis');
    expect(report.sourceCoverage?.[0]).toMatchObject({ analyzedDocuments: 1 });
  });
  it('keeps names in observations and preceding dialogue context without attributing other speakers', async () => {
    const original = 'Ada Lovelace: спроси @boris про https://example.org/plan';
    const parent = {
      ...sourceDocument('d2', 'Борис: Ada Lovelace, что делать?'),
      authorship: 'other' as const,
      authorId: 'author-boris',
      authorLabel: 'Борис',
    };
    const provider = analyzer((value) => {
      value.character = 'Ada Lovelace обсуждает решение с Борисом';
      (value.evidence as Record<string, unknown>[])[0]!.summary = original;
    });
    const { report } = await analyzeSourceCollections(
      sourceInput,
      [collection([{ ...sourceDocument('d1', original), replyTo: parent.id }, parent])],
      provider,
    );
    expect(report.character).toBe('Ada Lovelace обсуждает решение с Борисом');
    expect(report.evidence[0]?.summary).toBe(original);
    expect(report.styleExemplars).toEqual([
      expect.objectContaining({ text: original, context: parent.text, redactions: [] }),
    ]);
  });
  it.each(['unknown-reference', 'changed-quote'])(
    'rejects unsupported source assertions: %s',
    async (mode) => {
      const provider = analyzer((value) => {
        const evidence = (value.evidence as Record<string, unknown>[])[0]!;
        if (mode === 'unknown-reference') evidence.documentIds = ['missing-document'];
        else evidence.excerpt = 'MODEL INVENTED WORDS';
      });
      await expect(analyzeSourceCollections(sourceInput, [collection()], provider)).rejects.toThrow(
        /document|excerpt/i,
      );
    },
  );
  it('preserves original identity and formatting only from owned unforwarded reference examples', async () => {
    const original = '  Ada Lovelace!!\n\nну да)) 🙂  ';
    const documents: SourceDocument[] = [
      sourceDocument('d1', original),
      { ...sourceDocument('d2', 'other voice'), authorship: 'other', authorId: 'somebody' },
      { ...sourceDocument('d3', 'forwarded voice'), isForwarded: true },
      { ...sourceDocument('d4', 'held out voice'), partition: 'holdout' },
    ];
    const provider = analyzer();
    const { report } = await analyzeSourceCollections(
      sourceInput,
      [collection(documents)],
      provider,
    );
    expect(report.styleExemplars).toHaveLength(1);
    expect(report.styleExemplars?.[0]).toMatchObject({
      text: original,
      documentId: 'd1',
      exportAllowed: false,
      redactions: [],
    });
    const modelInput: unknown = JSON.parse(provider.run.mock.calls[0]![0].task);
    expect(modelInput).toHaveProperty(
      ['documents', 0],
      expect.objectContaining({ text: original, authorId: 'author-ada' }),
    );
    expect(JSON.stringify(modelInput)).not.toContain('held out voice');
  });
  it('analyzes all reference chunks and accounts for budget-limited unread documents', async () => {
    const documents = [
      sourceDocument('d1', 'x'.repeat(20000)),
      sourceDocument('d2', 'y'.repeat(20000)),
      sourceDocument('d3', 'z'.repeat(20000)),
    ];
    const provider = analyzer();
    const { report } = await analyzeSourceCollections(
      sourceInput,
      [collection(documents)],
      provider,
    );
    expect(provider.run).toHaveBeenCalledTimes(3);
    expect(report.sourceCoverage?.[0]).toMatchObject({
      analyzedDocuments: 3,
      coverage: { notAnalyzed: 0 },
    });
    const limited = collection(documents);
    const exhausted = {
      ...limited,
      usage: { ...limited.usage, steps: sourceGrant.budget.maxSteps },
    };
    const otherProvider = analyzer();
    const partial = await analyzeSourceCollections(sourceInput, [exhausted], otherProvider);
    expect(otherProvider.run).not.toHaveBeenCalled();
    expect(partial.report.sourceCoverage?.[0]).toMatchObject({
      analyzedDocuments: 0,
      coverage: { notAnalyzed: 3 },
    });
    expect(partial.report.evidence).toEqual([]);
  });
  it('excludes reference copies of held-out text before analysis and exemplar selection', async () => {
    const duplicate = { ...sourceDocument('d2', 'hidden reply'), partition: 'holdout' as const };
    const reference = {
      ...sourceDocument('d1', 'hidden reply'),
      contentHash: duplicate.contentHash,
    };
    const provider = analyzer();
    const { report } = await analyzeSourceCollections(
      sourceInput,
      [collection([duplicate, reference])],
      provider,
    );
    expect(provider.run).not.toHaveBeenCalled();
    expect(report.styleExemplars).toEqual([]);
  });
  it('honors a persisted native budget exhausted by an earlier run before sending a corpus chunk', async () => {
    const provider = analyzer();
    const reserveModelCall = vi.fn().mockRejectedValue(new Error('source-budget-exceeded'));
    const { report } = await runAnalysis(sourceInput, [collection()], provider, {
      ...analysisPort,
      reserveModelCall,
    });
    expect(reserveModelCall).toHaveBeenCalledOnce();
    expect(provider.run).not.toHaveBeenCalled();
    expect(report.sourceCoverage?.[0]).toMatchObject({
      analyzedDocuments: 0,
      coverage: { notAnalyzed: 1 },
    });
  });
  it('rejects a non-string status instead of coercing it into a supported observation', async () => {
    const provider = analyzer((value) => {
      (value.evidence as Record<string, unknown>[])[0]!.status = ['supported'];
    });
    await expect(analyzeSourceCollections(sourceInput, [collection()], provider)).rejects.toThrow(
      /status/i,
    );
  });
});
describe('source model context', () => {
  it('preserves names, URLs, handles and exact Unicode while retaining attribution', () => {
    const raw = '🙂 Ada Lovelace\n\n@ada, e\u0301  ada@example.org https://example.org/ada';
    const document = sourceDocument('d1', raw);
    expect(sourceModelDocument(document)).toMatchObject({
      text: raw,
      authorId: document.authorId,
      authorLabel: document.authorLabel,
      sourceUrl: document.sourceUrl,
      createdAt: document.createdAt,
    });
  });
});
