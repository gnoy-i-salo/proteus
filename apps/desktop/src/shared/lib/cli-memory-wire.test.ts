import { describe, expect, it } from 'vitest';
import { encodeCliRequest, encodeLegacyCliRequest } from './cli-wire-codec';

const request = {
  id: 'run-memory',
  provider: 'codex',
  model: 'fixture-model',
  effort: 'medium',
  task: '  Исходный контекст\r\n😀',
  systemPrompt: '  prompt\t',
  sourceContext: true,
  operation: 'corpus-analysis',
  memoryContext: { sessionId: 'memorySession:opaque-id', leaseId: 'modelContext:opaque-id' },
};

describe('CLI memory context reference', () => {
  it('preserves the exact input and both opaque native handles', () => {
    expect(encodeCliRequest(request)).toEqual(request);
    expect(encodeCliRequest({ ...request, operation: 'persona-reply' })).toEqual({
      ...request,
      operation: 'persona-reply',
    });
    expect(encodeCliRequest({ ...request, operation: 'generation' })).toEqual({
      ...request,
      operation: 'generation',
    });
  });
  it('never drops a memory reference through the legacy writer', () => {
    expect(() => encodeLegacyCliRequest(request)).toThrow();
  });
  it.each([
    null,
    {},
    { sessionId: 's' },
    { sessionId: '', leaseId: 'l' },
    { sessionId: 's', leaseId: 'a'.repeat(129) },
    { sessionId: ' s ', leaseId: 'l' },
    { sessionId: 's', leaseId: 'l\n' },
    { sessionId: 's', leaseId: 'l', personaId: 'foreign' },
    JSON.parse('{"sessionId":"s","leaseId":"l","session_id":"legacy"}') as unknown,
  ])('rejects malformed or extended native handles (%#)', (memoryContext) => {
    expect(() => encodeCliRequest({ ...request, memoryContext })).toThrow();
  });
  it.each([
    { sourceContext: false },
    { operation: 'fabrication' },
    { operation: 'public-research' },
    { operation: 'source-plan' },
  ])('rejects a context on an unrelated or unmarked invocation (%#)', (change) => {
    expect(() => encodeCliRequest({ ...request, ...change })).toThrow();
  });
});
