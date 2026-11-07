import { describe, expect, it } from 'vitest';
import { parsePublicReport } from './parse-public-report';

const excerpt = '  Анна → Борис\r\n«Ёлка» 👩🏽‍🔬\nhttps://example.org/Exact-ID  ';
const evidence = {
  id: 'e1',
  kind: 'fact',
  status: 'supported',
  summary: 'Анна отвечает Борису',
  sourceUrl: 'https://example.org/Exact-ID',
  excerpt,
  limitations: [],
};
const report = {
  evidence: [evidence],
  character: 'unknown',
  thinkingStyle: 'unknown',
  activity: 'unknown',
  behaviorSummary: 'unknown',
};

describe('public report parsing', () => {
  it('keeps exact attributed source text and never upgrades model claims into retrieval receipts', () => {
    const parsed = parsePublicReport({
      ...report,
      evidence: [
        { ...evidence, verification: 'source-retrieved', retrievedAt: '2026-01-01T00:00:00Z' },
      ],
    });
    expect(parsed.evidence[0]).toEqual({
      ...evidence,
      verification: 'model-reported',
      retrievedAt: null,
    });
    expect(parsed.evidence[0]?.excerpt).toBe(excerpt);
  });
  it.each([
    { items: [{ ...evidence, sourceUrl: 'http://localhost/private' }] },
    { items: [{ ...evidence, excerpt: null }] },
    { items: [{ ...evidence, status: ['supported'] }] },
    { items: [evidence, evidence] },
  ])('rejects invalid source scope, support or IDs before orchestration', ({ items }) => {
    expect(() => parsePublicReport({ ...report, evidence: items })).toThrow();
  });
});
