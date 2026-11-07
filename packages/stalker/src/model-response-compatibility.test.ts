import { describe, expect, it, vi } from 'vitest';
import type { IntelligenceProvider } from '@proteus-int/intelligence';
import type { SourcePort } from '@proteus-int/contracts';
import { collectPerson, sourceSkill } from './index';
import { analyzeSourceCollections } from './source-analysis';
import { parseCollectionAction } from './collection-action';
import { sourceDocument, sourceGrant, sourceInput, sourceResult } from './testing/source-fixtures';

const excerpt = '  Анна → Борис\r\n«Ёлка» 👩🏽‍🔬\nhttps://example.org/Exact-ID  ';
const canonicalEvidence = {
  id: 'e1',
  kind: 'fact',
  status: 'supported',
  summary: 'Анна отвечает Борису',
  sourceUrl: 'https://example.org/Exact-ID',
  excerpt,
  limitations: [],
};
const legacyEvidence = {
  id: 'e1',
  kind: 'fact',
  status: 'supported',
  summary: canonicalEvidence.summary,
  'source-url': canonicalEvidence.sourceUrl,
  excerpt,
  limitations: [],
};
const canonicalReport = {
  evidence: [canonicalEvidence],
  character: 'unknown',
  thinkingStyle: 'unknown',
  activity: 'unknown',
  behaviorSummary: 'unknown',
};
const legacyReport = {
  evidence: [legacyEvidence],
  character: 'unknown',
  'thinking-style': 'unknown',
  activity: 'unknown',
  'behavior-summary': 'unknown',
};

function provider(value: Record<string, unknown>) {
  const run = vi.fn<IntelligenceProvider['run']>().mockImplementation(() =>
    Promise.resolve({
      answer: JSON.stringify(value),
      trace: { provider: 'codex', reasoningMs: 1, terminalMs: 0, summary: 'done' },
    }),
  );
  return { id: 'codex' as const, run };
}

describe('STALKER model response schema compatibility', () => {
  it.each([
    ['legacy', legacyReport],
    ['canonical', canonicalReport],
  ] as const)(
    'reads %s public evidence into canonical fields without changing exact excerpts',
    async (_schema, response) => {
      const model = provider(response);
      const { report } = await collectPerson(sourceInput, model);
      expect(report.evidence[0]).toMatchObject({
        sourceUrl: canonicalEvidence.sourceUrl,
        excerpt,
        verification: 'model-reported',
        retrievedAt: null,
      });
      expect(report.evidence[0]).not.toHaveProperty('source-url');
      expect(report).toMatchObject({ thinkingStyle: 'unknown', behaviorSummary: 'unknown' });
      expect(model.run.mock.calls[0]?.[0].systemPrompt).toContain('sourceUrl');
      expect(model.run.mock.calls[0]?.[0].systemPrompt).toContain('thinkingStyle');
      expect(model.run.mock.calls[0]?.[0].systemPrompt).not.toContain('"source-url"');
    },
  );

  it('keeps the full original public excerpt including edge whitespace', async () => {
    const { report } = await collectPerson(sourceInput, provider(legacyReport));
    expect(report.evidence[0]?.excerpt).toBe(excerpt);
  });

  it.each([
    { ...legacyReport, thinkingStyle: 'unknown' },
    {
      ...canonicalReport,
      evidence: [{ ...legacyEvidence, sourceUrl: canonicalEvidence.sourceUrl }],
    },
  ])('rejects mixed root or evidence aliases even when values are identical', async (response) => {
    await expect(collectPerson(sourceInput, provider(response))).rejects.toThrow(/mixed/i);
  });

  it.each(['', ' '.repeat(3), 'x'.repeat(24001)])(
    'still rejects invalid excerpt length/content',
    async (value) => {
      await expect(
        collectPerson(
          sourceInput,
          provider({ ...canonicalReport, evidence: [{ ...canonicalEvidence, excerpt: value }] }),
        ),
      ).rejects.toThrow(/excerpt/i);
    },
  );

  it.each([
    { action: 'read', 'document-ids': ['d1'] },
    { action: 'read', documentIds: ['d1'] },
  ])('decodes source read actions into canonical IDs', (action) => {
    expect(parseCollectionAction(action)).toEqual({ action: 'read', documentIds: ['d1'] });
  });

  it('rejects mixed action aliases instead of silently choosing one', () => {
    expect(() =>
      parseCollectionAction({ action: 'read', 'document-ids': ['d1'], documentIds: ['d1'] }),
    ).toThrow(/mixed/i);
  });

  it.each(['telegram', 'vk', 'instagram', 'github'] as const)(
    'documents canonical read action fields in the %s source skill',
    (source) => {
      expect(sourceSkill(source)).toContain('`documentIds`');
      expect(sourceSkill(source)).not.toContain('`document-ids`');
      expect(parseCollectionAction({ action: 'read', documentIds: ['d1'] })).toEqual({
        action: 'read',
        documentIds: ['d1'],
      });
    },
  );

  it.each(['legacy', 'canonical'] as const)(
    'reads %s corpus evidence with exact native receipts',
    async (schema) => {
      const document = sourceDocument('d1', excerpt);
      const model = provider(
        schema === 'legacy'
          ? {
              ...legacyReport,
              evidence: [
                {
                  ...legacyEvidence,
                  'document-ids': ['d1'],
                  'source-url': 'https://invented.example',
                  'retrieved-at': '2099-01-01T00:00:00Z',
                },
              ],
            }
          : {
              ...canonicalReport,
              evidence: [
                {
                  ...canonicalEvidence,
                  documentIds: ['d1'],
                  sourceUrl: 'https://invented.example',
                  retrievedAt: '2099-01-01T00:00:00Z',
                },
              ],
            },
      );
      const port: SourcePort = {
        execute() {
          return Promise.reject(new Error('No extra source reads'));
        },
        async reserveModelCall() {},
      };
      const { report } = await analyzeSourceCollections(
        sourceInput,
        [
          {
            grant: sourceGrant,
            documents: [document],
            coverage: sourceResult([document]).coverage,
            traces: [],
            status: 'complete',
            limitations: [],
            usage: { steps: 1, bytes: 100, durationMs: 1 },
          },
        ],
        model,
        port,
      );
      expect(report.evidence[0]).toMatchObject({
        documentIds: ['d1'],
        excerpt,
        sourceUrl: document.sourceUrl,
        retrievedAt: document.retrievedAt,
        verification: 'source-retrieved',
      });
      expect(report.styleExemplars?.[0]?.text).toBe(excerpt);
      expect(model.run.mock.calls[0]?.[0].systemPrompt).toContain('documentIds');
      expect(model.run.mock.calls[0]?.[0].systemPrompt).not.toContain('"document-ids"');
    },
  );
});
