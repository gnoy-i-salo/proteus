import { describe, expect, it } from 'vitest';
import {
  decodeLegacyCliEvent,
  decodeLegacyCliResult,
  decodeLegacyCliTrace,
  encodeLegacyCliRequest,
} from './cli-wire-codec';

const publicText = '  Анна → Борис: «source-url»\r\n\n  👩🏽‍💻\t';
const reasoning = [{ id: 'block-1', kind: 'summary', text: publicText }];
const trace = {
  provider: 'codex',
  'wall-time-ms': 123,
  'reasoning-ms': 123,
  'active-reasoning-ms': null,
  'requested-model': 'configured-alias',
  'resolved-model': null,
  'requested-effort': 'high',
  'applied-effort': 'high',
  'cli-version': null,
  'terminal-ms': 0,
  summary: publicText,
  reasoning,
  'reasoning-truncated': true,
};
const request = {
  id: 'run-1',
  provider: 'codex' as const,
  model: 'model-alias',
  task: publicText,
  systemPrompt: 'PERSON\nОтветь от первого лица.',
  effort: 'high' as const,
};

describe('legacy CLI boundary', () => {
  it('maps an actual native trace without inventing observed metadata or modifying text', () => {
    expect(decodeLegacyCliTrace(trace)).toEqual({
      provider: 'codex',
      wallTimeMs: 123,
      reasoningMs: 123,
      activeReasoningMs: null,
      requestedModel: 'configured-alias',
      resolvedModel: null,
      requestedEffort: 'high',
      appliedEffort: 'high',
      cliVersion: null,
      terminalMs: 0,
      summary: publicText,
      reasoning,
      reasoningTruncated: true,
    });
  });

  it('preserves missing optional legacy metadata separately from explicit null', () => {
    const decoded = decodeLegacyCliTrace({
      provider: 'kimi',
      'reasoning-ms': 42,
      'terminal-ms': 0,
      summary: '',
    });
    expect(decoded).toEqual({ provider: 'kimi', reasoningMs: 42, terminalMs: 0, summary: '' });
    expect(Object.hasOwn(decoded, 'wallTimeMs')).toBe(false);
    expect(Object.hasOwn(decoded, 'reasoning')).toBe(false);
    expect(Object.hasOwn(decoded, 'resolvedModel')).toBe(false);
  });

  it('preserves observed values and keeps answer text outside reasoning', () => {
    const answer = '{"source-url":"https://example.org/a-b","text":"  Анна  "}';
    expect(
      decodeLegacyCliResult({
        answer,
        trace: { ...trace, 'resolved-model': 'observed', 'cli-version': '1.2.3', reasoning: [] },
      }),
    ).toMatchObject({
      answer,
      trace: { resolvedModel: 'observed', cliVersion: '1.2.3', reasoning: [] },
    });
    expect(trace.reasoning[0]?.text).toBe(publicText);
  });

  it.each([
    { ...trace, wallTimeMs: 123 },
    { ...trace, wallTimeMs: 999 },
    { ...trace, version: 2 },
    { ...trace, provider: 'unknown' },
    { ...trace, 'reasoning-ms': -1 },
    { ...trace, 'wall-time-ms': Infinity },
    { ...trace, 'terminal-ms': 0.5 },
    { ...trace, 'active-reasoning-ms': 100 },
    { ...trace, 'requested-effort': 'maximum' },
    { ...trace, 'applied-effort': undefined },
    { ...trace, 'resolved-model': 123 },
    { ...trace, reasoning: null },
    { ...trace, reasoning: [{ id: 'x', kind: 'answer', text: 'Not reasoning' }] },
    { ...trace, reasoning: [{ id: 'x', kind: 'thinking', text: 'Exposed', raw: 'hidden' }] },
    { ...trace, reasoning: [...reasoning, ...reasoning] },
    { ...trace, 'reasoning-truncated': 'false' },
  ])('rejects invalid or mixed traces without coercion: %j', (value) => {
    expect(() => decodeLegacyCliTrace(value)).toThrow();
  });

  it('rejects oversized reasoning instead of silently changing source text', () => {
    expect(() =>
      decodeLegacyCliTrace({
        ...trace,
        reasoning: Array.from({ length: 65 }, (_, i) => ({
          id: String(i),
          kind: 'summary',
          text: 'x',
        })),
      }),
    ).toThrow();
    expect(() =>
      decodeLegacyCliTrace({
        ...trace,
        reasoning: [{ id: 'x', kind: 'summary', text: 'я'.repeat(24577) }],
      }),
    ).toThrow();
  });

  it.each([null, [], {}, { answer: 'answer', trace, stderr: 'private' }, { answer: null, trace }])(
    'rejects malformed result envelopes: %j',
    (value) => expect(() => decodeLegacyCliResult(value)).toThrow(),
  );

  it('decodes public progress while preserving absent reasoning and event kind', () => {
    for (const kind of ['started', 'answer', 'search', 'finished']) {
      const event = decodeLegacyCliEvent({ id: 'run-1', kind, 'elapsed-ms': 5 });
      expect(event).toEqual({ id: 'run-1', kind, elapsedMs: 5 });
      expect(Object.hasOwn(event, 'reasoning')).toBe(false);
    }
    expect(
      decodeLegacyCliEvent({
        id: 'run-1',
        kind: 'reasoning',
        'elapsed-ms': 7,
        reasoning: reasoning[0],
      }),
    ).toEqual({ id: 'run-1', kind: 'reasoning', elapsedMs: 7, reasoning: reasoning[0] });
  });

  it.each([
    { id: 'run-1', kind: 'started', 'elapsed-ms': 0, elapsedMs: 0 },
    { id: 'run-1', kind: 'reasoning', 'elapsed-ms': 0 },
    { id: 'run-1', kind: 'answer', 'elapsed-ms': 0, reasoning: reasoning[0] },
    { id: 'run-1', kind: 'started', 'elapsed-ms': 0, reasoning: null },
    { id: 'run-1', kind: 'raw', 'elapsed-ms': 0 },
    { id: '', kind: 'started', 'elapsed-ms': 0 },
    { id: 'run-1', kind: 'started', 'elapsed-ms': NaN },
    { id: 'run-1', kind: 'started', 'elapsed-ms': 0, answer: 'Not reasoning' },
  ])('rejects invalid or ambiguous event schemas: %j', (value) => {
    expect(() => decodeLegacyCliEvent(value)).toThrow();
  });

  it('encodes only owned request fields and keeps domain repair metadata off the native wire', () => {
    expect(
      encodeLegacyCliRequest({
        ...request,
        sourceContext: true,
        operation: 'persona-reply',
        formatAttempt: 2,
      }),
    ).toEqual({
      id: 'run-1',
      provider: 'codex',
      model: 'model-alias',
      task: publicText,
      'system-prompt': request.systemPrompt,
      effort: 'high',
      'source-context': true,
      operation: 'persona-reply',
    });
    const encoded = encodeLegacyCliRequest({
      id: 'run-2',
      provider: 'kimi',
      task: '',
      systemPrompt: '',
      effort: 'low',
    });
    expect(encoded).toEqual({
      id: 'run-2',
      provider: 'kimi',
      model: '',
      task: '',
      'system-prompt': '',
      effort: 'low',
    });
    expect(Object.hasOwn(encoded, 'source-context')).toBe(false);
    expect(Object.hasOwn(encoded, 'operation')).toBe(false);
  });

  it.each([
    { ...request, 'system-prompt': request.systemPrompt },
    { ...request, schemaVersion: 2 },
    { ...request, id: '../run' },
    { ...request, model: 'bad\nmodel' },
    { ...request, model: 'я'.repeat(61) },
    { ...request, sourceContext: null },
    { ...request, formatAttempt: 3 },
    { ...request, operation: 'unknown' },
    { ...request, task: 'я'.repeat(100001) },
    { ...request, systemPrompt: 'я'.repeat(20001) },
  ])('rejects unsupported canonical requests instead of passing ambiguous data', (value) => {
    expect(() => encodeLegacyCliRequest(value)).toThrow();
  });
});
