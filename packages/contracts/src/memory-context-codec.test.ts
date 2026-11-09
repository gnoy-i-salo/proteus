import { describe, expect, it } from 'vitest';
import { decodeModelContextRequest } from './memory-context-codec';

const request = () => ({
  schemaVersion: 2,
  purpose: 'research',
  manifestId: null,
  systemPrompt: '  Инструкция\r\n👩🏽‍🔬',
  task: '\uFEFFЗадача\r\n  ',
  receiptIds: ['retrieval:1'],
  memoryItemIds: [],
  formatAttempt: 1,
});
describe('native model context request', () => {
  it('preserves exact base input and accepts a second format attempt', () => {
    const input = request();
    const before = structuredClone(input);
    expect(decodeModelContextRequest(input)).toEqual(input);
    expect(input).toEqual(before);
    expect(decodeModelContextRequest({ ...input, formatAttempt: 2 }).formatAttempt).toBe(2);
    expect(decodeModelContextRequest({ ...input, systemPrompt: '', task: '' }).task).toBe('');
  });
  it.each([
    { schemaVersion: 1 },
    { schemaVersion: 3 },
    { inputSha256: 'a'.repeat(64) },
    { inputBytes: 10 },
    { task: 42 },
    { systemPrompt: null },
    { formatAttempt: 0 },
    { formatAttempt: 3 },
    { purpose: 'arbitrary' },
    { manifestId: 'manifest:foreign' },
    { memoryItemIds: ['item:1'] },
    { receiptIds: ['retrieval:1', 'retrieval:1'] },
    { receiptIds: [''] },
    { receiptIds: ['id\n'] },
    { receiptIds: ['я'.repeat(129)] },
    { receiptIds: Array.from({ length: 1001 }, (_, i) => `r:${i}`) },
  ])('rejects unsupported or conflicting fields without changing originals: %j', (patch) => {
    const input = { ...request(), ...patch };
    const before = structuredClone(input);
    expect(() => decodeModelContextRequest(input)).toThrow();
    expect(input).toEqual(before);
  });
  it('requires every own field, including explicit nullable manifestId', () => {
    for (const field of Object.keys(request())) {
      const input: Record<string, unknown> = { ...request() };
      delete input[field];
      expect(() => decodeModelContextRequest(input)).toThrow();
    }
  });
  it('bounds UTF8 input bytes without trimming or normalizing text', () => {
    expect(
      decodeModelContextRequest({ ...request(), systemPrompt: 'я'.repeat(20000) }).systemPrompt
        .length,
    ).toBe(20000);
    expect(() =>
      decodeModelContextRequest({ ...request(), systemPrompt: 'я'.repeat(20001) }),
    ).toThrow();
    expect(decodeModelContextRequest({ ...request(), task: '😀'.repeat(50000) }).task.length).toBe(
      100000,
    );
    expect(() => decodeModelContextRequest({ ...request(), task: '😀'.repeat(50001) })).toThrow();
  });
  it('requires an explicit frozen manifest for persona replies', () => {
    const input = {
      ...request(),
      purpose: 'personaReply',
      manifestId: 'manifest:1',
      memoryItemIds: ['item:1'],
    };
    expect(decodeModelContextRequest(input)).toEqual(input);
    expect(() => decodeModelContextRequest({ ...input, manifestId: null })).toThrow();
    expect(() =>
      decodeModelContextRequest({ ...input, memoryItemIds: ['item:1', 'item:1'] }),
    ).toThrow();
  });
  it('rejects unpaired surrogates instead of replacing original text at the Rust boundary', () => {
    for (const field of ['systemPrompt', 'task']) {
      for (const invalid of ['\ud800', '\udfff', 'before\ud800after']) {
        expect(() => decodeModelContextRequest({ ...request(), [field]: invalid })).toThrow();
      }
    }
    expect(() => decodeModelContextRequest({ ...request(), receiptIds: ['r:\ud800'] })).toThrow();
    const exact = '\uFEFF\uFFFD 👩🏽‍🔬\r\n';
    expect(decodeModelContextRequest({ ...request(), task: exact }).task).toBe(exact);
  });
});
