import { describe, expect, it, vi } from 'vitest';
import type { PersonInput, SourceDocument, SourcePort } from '@proteus-int/contracts';
import type { IntelligenceProvider } from '@proteus-int/intelligence';
import { analyzeSourceCollections as runAnalysis } from './source-analysis';
import { sourceDocument, sourceGrant, sourceInput, sourceResult } from './source-fixtures';
import { sanitizeSourceText } from './source-sanitizer';
import { collectSource, type SourceCollection } from './source-controller';

const analysisPort: SourcePort = {
  async execute() {
    throw new Error('Analysis must not read sources');
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
    usage: { steps: 2, bytes: 80, 'duration-ms': 1 },
  };
}
function analyzer(change?: (value: Record<string, unknown>) => void) {
  const run = vi.fn<IntelligenceProvider['run']>().mockImplementation(async (request) => {
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
          'document-ids': [doc.id],
          excerpt: doc.text.slice(0, 60),
          'retrieved-at': '2099-01-01T00:00:00Z',
          'source-url': 'https://invented.example/profile',
          limitations: [],
        })),
      character: 'unknown',
      'thinking-style': 'Задаёт уточняющие вопросы',
      activity: 'unknown',
      'behavior-summary': 'unknown',
    };
    change?.(value);
    return {
      answer: JSON.stringify(value),
      trace: { provider: 'codex', 'reasoning-ms': 1, 'terminal-ms': 0, summary: 'done' },
    };
  });
  return { id: 'codex' as const, run };
}
describe('source corpus analysis', () => {
  it.each([3, 40])(
    'keeps analysis and one format repair available after a long source walk with %i steps',
    async (maxSteps) => {
      const grant = { ...sourceGrant, budget: { ...sourceGrant.budget, 'max-steps': maxSteps } };
      let spent = 0;
      let pages = 0;
      const reserve = async () => {
        if (spent >= maxSteps) throw new Error('source-budget-exceeded');
        spent++;
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
        trace: { provider: 'codex', 'reasoning-ms': 1, 'terminal-ms': 0, summary: 'retry' },
      });
      const collected = await collectSource(grant, sourceInput, provider, port);
      const { report } = await runAnalysis(sourceInput, [collected], provider, port);
      expect(provider.run.mock.calls.length).toBeGreaterThanOrEqual(2);
      expect(provider.run.mock.calls[1]?.[0]['format-attempt']).toBe(2);
      expect(
        provider.run.mock.calls.every(([request]) => request.operation === 'corpus-analysis'),
      ).toBe(true);
      expect(collected.status).toBe('partial');
      expect(collected.coverage.complete).toBe(false);
      expect(collected.usage.steps).toBe(pages);
      expect(collected.usage.steps).toBeLessThanOrEqual(maxSteps - 2);
      expect(collected.limitations.join(' ')).toMatch(/analysis.*budget|budget.*analysis/i);
      expect(report['source-coverage']?.[0]).toMatchObject({ 'analyzed-documents': pages });
      expect(spent).toBe(pages + provider.run.mock.calls.length);
      expect(spent).toBeLessThanOrEqual(maxSteps);
    },
  );
  it('allows a one-step grant to read once and reports that analysis could not run', async () => {
    const grant = { ...sourceGrant, budget: { ...sourceGrant.budget, 'max-steps': 1 } };
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
    expect(report['source-coverage']?.[0]).toMatchObject({
      'analyzed-documents': 0,
      coverage: { 'not-analyzed': 1 },
    });
    expect(report['source-coverage']?.[0]?.limitations.join(' ')).toMatch(/analysis budget/i);
  });
  it('grounds facts in exact native text and never accepts model retrieval metadata', async () => {
    const provider = analyzer();
    const { report } = await analyzeSourceCollections(sourceInput, [collection()], provider);
    expect(report.evidence[0]).toMatchObject({
      verification: 'source-retrieved',
      'retrieved-at': '2026-02-01T00:00:00Z',
      'source-url': 'https://example.org/ada',
      'document-ids': ['d1'],
    });
    expect(provider.run.mock.calls[0]?.[0]).toHaveProperty('operation', 'corpus-analysis');
    expect(report['source-coverage']?.[0]).toMatchObject({ 'analyzed-documents': 1 });
  });
  it.each(['unknown-reference', 'changed-quote'])(
    'rejects unsupported source assertions: %s',
    async (mode) => {
      const provider = analyzer((value) => {
        const evidence = (value.evidence as Record<string, unknown>[])[0]!;
        if (mode === 'unknown-reference') evidence['document-ids'] = ['missing-document'];
        else evidence.excerpt = 'MODEL INVENTED WORDS';
      });
      await expect(analyzeSourceCollections(sourceInput, [collection()], provider)).rejects.toThrow(
        /document|excerpt/i,
      );
    },
  );
  it('preserves exact sanitized formatting only from owned unforwarded reference examples', async () => {
    const original = '  Ada Lovelace!!\n\nну да)) 🙂  ';
    const documents: SourceDocument[] = [
      sourceDocument('d1', original),
      { ...sourceDocument('d2', 'other voice'), authorship: 'other', 'author-id': 'somebody' },
      { ...sourceDocument('d3', 'forwarded voice'), 'is-forwarded': true },
      { ...sourceDocument('d4', 'held out voice'), partition: 'holdout' },
    ];
    const provider = analyzer();
    const { report } = await analyzeSourceCollections(
      sourceInput,
      [collection(documents)],
      provider,
    );
    expect(report['style-exemplars']).toHaveLength(1);
    expect(report['style-exemplars']?.[0]).toMatchObject({
      text: '  [person]!!\n\nну да)) 🙂  ',
      'document-id': 'd1',
      'export-allowed': false,
      redactions: [{ start: 2, end: 10 }],
    });
    expect(provider.run.mock.calls.map((call) => call[0].task).join(' ')).not.toMatch(
      /Ada Lovelace|held out voice|private-chat|author-ada/,
    );
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
    expect(report['source-coverage']?.[0]).toMatchObject({
      'analyzed-documents': 3,
      coverage: { 'not-analyzed': 0 },
    });
    const limited = collection(documents);
    const exhausted = {
      ...limited,
      usage: { ...limited.usage, steps: sourceGrant.budget['max-steps'] },
    };
    const otherProvider = analyzer();
    const partial = await analyzeSourceCollections(sourceInput, [exhausted], otherProvider);
    expect(otherProvider.run).not.toHaveBeenCalled();
    expect(partial.report['source-coverage']?.[0]).toMatchObject({
      'analyzed-documents': 0,
      coverage: { 'not-analyzed': 3 },
    });
    expect(partial.report.evidence).toEqual([]);
  });
  it('excludes reference copies of held-out text before analysis and exemplar selection', async () => {
    const duplicate = { ...sourceDocument('d2', 'hidden reply'), partition: 'holdout' as const };
    const reference = {
      ...sourceDocument('d1', 'hidden reply'),
      'content-hash': duplicate['content-hash'],
    };
    const provider = analyzer();
    const { report } = await analyzeSourceCollections(
      sourceInput,
      [collection([duplicate, reference])],
      provider,
    );
    expect(provider.run).not.toHaveBeenCalled();
    expect(report['style-exemplars']).toEqual([]);
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
    expect(report['source-coverage']?.[0]).toMatchObject({
      'analyzed-documents': 0,
      coverage: { 'not-analyzed': 1 },
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
describe('format-preserving source pseudonyms', () => {
  it('tracks each replacement in the resulting UTF-16 text without normalizing Unicode or whitespace', () => {
    const raw = '🙂 Ada Lovelace\n\n@ada, e\u0301  Ada Lovelace';
    const safe = sanitizeSourceText(raw, sourceInput, sourceGrant, []);
    expect(safe.text).toBe('🙂 [person]\n\n[person], e\u0301  [person]');
    expect(safe.replacements.map(({ start, end }) => safe.text.slice(start, end))).toEqual([
      '[person]',
      '[person]',
      '[person]',
    ]);
  });
});
