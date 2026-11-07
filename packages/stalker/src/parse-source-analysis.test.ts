import { describe, expect, it } from 'vitest';
import { parseAnalysis } from './parse-source-analysis';
import { sourceModelDocument } from './source-model-document';
import { sourceDocument } from './testing/source-fixtures';

const document = sourceDocument('d1', '  Анна → Борис\r\n«Ёлка» 👩🏽‍🔬  ');
const evidence = {
  id: 'e1',
  kind: 'fact',
  status: 'supported',
  summary: 'Анна отвечает Борису',
  documentIds: ['d1'],
  excerpt: document.text,
  limitations: [],
  sourceUrl: 'https://invented.example',
  retrievedAt: '2099-01-01T00:00:00Z',
};
const report = {
  evidence: [evidence],
  character: 'unknown',
  thinkingStyle: 'unknown',
  activity: 'unknown',
  behaviorSummary: 'unknown',
};

describe('source analysis parsing', () => {
  it('uses the native provenance and exact source text without claiming the assertion is verified', () => {
    const parsed = parseAnalysis(report, [document], [sourceModelDocument(document)]);
    expect(parsed.evidence[0]).toMatchObject({
      excerpt: document.text,
      documentIds: ['d1'],
      sourceUrl: document.sourceUrl,
      retrievedAt: document.retrievedAt,
      verification: 'source-retrieved',
    });
    expect(parsed.evidence[0]?.limitations).toContain(
      'Source retrieval does not independently verify the assertion',
    );
  });
  it.each([
    { ...evidence, excerpt: 'paraphrased' },
    { ...evidence, documentIds: ['unknown'] },
    { ...evidence, documentIds: ['d1', 'd1'] },
    { ...evidence, documentIds: [] },
  ])('rejects invented quotations and document references without calling a provider', (item) => {
    expect(() =>
      parseAnalysis({ ...report, evidence: [item] }, [document], [sourceModelDocument(document)]),
    ).toThrow();
  });
  it.each([
    { ...document, authorship: 'other' as const },
    { ...document, authorship: 'unknown' as const },
    { ...document, isForwarded: true },
  ])('does not attribute another speaker or forwarded text to the selected author', (item) => {
    expect(() => parseAnalysis(report, [item], [sourceModelDocument(item)])).toThrow(/authored/);
  });
});
