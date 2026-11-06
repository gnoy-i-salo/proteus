import { describe, expect, it } from 'vitest';
import type { IntelligenceProvider } from '@proteus-int/intelligence';
import { anonymizeReport, collectPerson, redactTask } from './index';

const input = {
  label: 'Ada Lovelace',
  context: 'Prefers checking assumptions; contact ada@example.org',
  'public-urls': ['https://example.org/people/ada-lovelace'],
};
const evidence = {
  id: 'e1',
  kind: 'fact',
  'source-url': 'https://example.org/people/ada-lovelace',
  summary: 'Ada Lovelace checks assumptions',
  status: 'supported',
  verification: 'verified',
  excerpt: 'Checks assumptions before choosing',
  'retrieved-at': '2026-01-01T00:00:00Z',
  limitations: ['One reported example'],
};
function provider(items: unknown[]): IntelligenceProvider {
  return {
    id: 'codex',
    async run() {
      return {
        answer: JSON.stringify({
          evidence: items,
          character: 'unknown',
          'thinking-style': 'unknown',
          activity: 'unknown',
          'behavior-summary': 'unknown',
        }),
        trace: { provider: 'codex', 'reasoning-ms': 1, 'terminal-ms': 0, summary: 'done' },
      };
    },
  };
}
describe('honest evidence provenance', () => {
  it('accepts unknown profiles without inventing a BSC and never trusts claimed verification', async () => {
    const { report } = await collectPerson(input, provider([evidence]));
    expect(report).not.toHaveProperty('estimated-bsc');
    expect(report.evidence[0]).toMatchObject({
      verification: 'model-reported',
      'retrieved-at': null,
      status: 'supported',
    });
    const empty = await collectPerson(input, provider([]));
    expect(empty.report.evidence).toEqual([]);
  });
  it('rejects duplicate references and unsupported supported claims', async () => {
    await expect(collectPerson(input, provider([evidence, evidence]))).rejects.toThrow(
      /duplicate/i,
    );
    await expect(collectPerson(input, provider([{ ...evidence, excerpt: null }]))).rejects.toThrow(
      /excerpt/i,
    );
  });
  it('rejects non-string evidence statuses instead of coercing them into supported claims', async () => {
    await expect(
      collectPerson(input, provider([{ ...evidence, status: ['supported'] }])),
    ).rejects.toThrow(/status/i);
  });
  it('keeps opaque references while removing identifiers from evidence and task inputs', async () => {
    const { report } = await collectPerson(input, provider([evidence]));
    const anonymous = anonymizeReport(report, input);
    expect(anonymous.evidence[0]).toMatchObject({ id: 'e1', status: 'supported' });
    expect(JSON.stringify(anonymous)).not.toMatch(/Ada Lovelace|example.org|ada-lovelace/i);
    expect(
      redactTask('Ask Ada Lovelace and @ada-lovelace at ada@example.org', [input]),
    ).not.toMatch(/Ada Lovelace|@ada-lovelace|ada@example.org/i);
  });
  it('preserves unrelated task links and handles while scrubbing persona references', () => {
    const task =
      'Ask Ada Lovelace at https://example.org/people/ada-lovelace?ref=task. Use https://standards.example/spec and @support-desk.';
    const clean = redactTask(task, [input]);
    expect(clean).toContain('https://standards.example/spec');
    expect(clean).toContain('@support-desk');
    expect(clean).not.toContain('Ada Lovelace');
    expect(clean).not.toContain('example.org/people/ada-lovelace');
  });
  it('still removes unrelated locations and handles from behavioral evidence', async () => {
    const { report } = await collectPerson(
      input,
      provider([
        {
          ...evidence,
          summary: 'Consults https://standards.example/spec and @support-desk',
        },
      ]),
    );
    expect(JSON.stringify(anonymizeReport(report, input))).not.toMatch(
      /standards.example|@support-desk/,
    );
  });
});
