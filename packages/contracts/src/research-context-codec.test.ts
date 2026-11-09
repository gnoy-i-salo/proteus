import { describe, expect, it } from 'vitest';
import { decodeResearchContext } from './research-context-codec';
import { projectBoundExcerpt } from './source-excerpt';
import { binding, revision } from './testing/memory-fixtures';
import type { ModelCallBudget, ModelContextLease, ModelContextRequest } from './memory';
import type { ResearchSessionDescriptor } from './research-memory';

const session: ResearchSessionDescriptor = {
  schemaVersion: 1,
  id: 'session:1',
  personaId: binding.personaId,
  corpusSnapshotId: 'snapshot:1',
  policyVersion: binding.policyVersion,
  mode: 'lexical',
  indexGeneration: 2,
  expiresAt: '2026-11-09T03:01:00.000Z',
};
const request: ModelContextRequest = {
  schemaVersion: 2,
  purpose: 'research',
  manifestId: null,
  systemPrompt: '\uFEFFТочный системный текст\r\n',
  task: '  Задача 👩🏽‍🔬\r\n',
  receiptIds: ['receipt:1'],
  memoryItemIds: [],
  formatAttempt: 1,
};
const budget: ModelCallBudget = { maxCalls: 3, maxInputBytes: 1048576, maxDurationMs: 60000 };
const sources = [
  projectBoundExcerpt(binding, revision, {
    documentId: revision.document.id,
    revisionId: revision.revisionId,
    unit: 'utf8Bytes',
    start: 0,
    end: new TextEncoder().encode(revision.document.text).length,
  }),
];
function response(): ModelContextLease {
  const task = request.task + '\nNative context: ' + JSON.stringify(sources);
  const inputBytes = new TextEncoder().encode(JSON.stringify([request.systemPrompt, task])).length;
  return {
    schemaVersion: 2,
    id: 'lease:1',
    singleUse: true,
    personaId: session.personaId,
    inputSha256: 'a'.repeat(64),
    inputBytes,
    systemPrompt: request.systemPrompt,
    task,
    corpusSnapshotId: session.corpusSnapshotId,
    manifestId: null,
    purpose: 'research',
    formatAttempt: 1,
    policyVersion: session.policyVersion,
    receiptIds: request.receiptIds,
    items: [],
    sources,
    contextSha256: 'b'.repeat(64),
    expiresAt: session.expiresAt,
    usage: { modelCalls: 1, inputBytes },
  };
}
function decode(value: unknown) {
  return decodeResearchContext(value, session, request, budget, sources);
}
describe('native research context response', () => {
  it('retains exact native input, provenance, usage and opaque digest without rebuilding a prompt', () => {
    const input = response();
    const original = JSON.stringify(input);
    expect(decode(input)).toEqual(input);
    expect(JSON.stringify(input)).toBe(original);
  });
  it.each([
    { schemaVersion: 1 },
    { singleUse: false },
    { personaId: 'foreign' },
    { corpusSnapshotId: 'foreign' },
    { policyVersion: 'foreign' },
    { purpose: 'personaReply' },
    { manifestId: 'manifest:1' },
    { formatAttempt: 2 },
    { receiptIds: ['foreign'] },
    { receiptIds: ['receipt:1', 'receipt:1'] },
    { items: [{}] },
    { systemPrompt: request.systemPrompt.trim() },
    { task: request.task },
    { task: 'foreign' },
    { expiresAt: '2026-11-09T03:02:00.000Z' },
    { expiresAt: '2026-11-09T03:01:00Z' },
    { id: '\n' },
    { inputBytes: 1 },
    { inputSha256: 'G'.repeat(64) },
    { contextSha256: 'a'.repeat(63) },
    { sources: [] },
    { unknown: true },
  ])('rejects a mismatched lease: %j', (patch) => {
    expect(() => decode({ ...response(), ...patch })).toThrow();
  });
  it('checks exact sources and their order, including original author labels', () => {
    const input = response();
    const source = sources[0]!;
    for (const changed of [
      { ...source, document: { ...source.document, authorLabel: 'other' } },
      { ...source, span: { ...source.span, revisionId: 'foreign' } },
      { ...source, text: source.text.replace('Анна', 'Боря') },
    ])
      expect(() => decode({ ...input, sources: [changed] })).toThrow();
    const second = { ...source, span: { ...source.span, revisionId: 'revision:2' } };
    expect(() =>
      decodeResearchContext({ ...input, sources: [second, source] }, session, request, budget, [
        source,
        second,
      ]),
    ).toThrow();
  });
  it('counts framed UTF8 bytes and rejects invalid Unicode or oversized input', () => {
    for (const task of [request.task + '\ud800', request.task + 'я'.repeat(100000)]) {
      const input = response();
      const inputBytes = new TextEncoder().encode(
        JSON.stringify([input.systemPrompt, task]),
      ).length;
      expect(() =>
        decode({ ...input, task, inputBytes, usage: { modelCalls: 1, inputBytes } }),
      ).toThrow();
    }
  });
  it('bounds cumulative native usage but allows an earlier lost reservation response', () => {
    const input = response();
    const usage = { modelCalls: 3, inputBytes: input.inputBytes * 3 };
    expect(decode({ ...input, usage }).usage).toEqual(usage);
    for (const usage of [
      { modelCalls: 0, inputBytes: input.inputBytes },
      { modelCalls: 4, inputBytes: input.inputBytes },
      { modelCalls: 1.5, inputBytes: input.inputBytes },
      { modelCalls: 1, inputBytes: input.inputBytes - 1 },
      { modelCalls: 1, inputBytes: budget.maxInputBytes + 1 },
      { modelCalls: 1, inputBytes: input.inputBytes, unknown: 1 },
    ])
      expect(() => decode({ ...input, usage })).toThrow();
  });
  it('accepts a second format attempt only when requested and never admits a persona context', () => {
    const input = { ...response(), formatAttempt: 2 };
    expect(
      decodeResearchContext(input, session, { ...request, formatAttempt: 2 }, budget, sources),
    ).toEqual(input);
    expect(() =>
      decodeResearchContext(
        response(),
        session,
        { ...request, purpose: 'personaReply', manifestId: 'manifest:1' },
        budget,
        sources,
      ),
    ).toThrow();
  });
});
