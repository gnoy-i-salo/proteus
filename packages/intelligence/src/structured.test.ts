import { describe, expect, it, vi } from 'vitest';
import { structured, text } from './structured';
import type { IntelligenceProvider } from './provider';
const trace = { provider: 'codex' as const, 'reasoning-ms': 1, 'terminal-ms': 0, summary: '' };
const decode = (data: Record<string, unknown>) => ({ answer: text(data.answer, 'answer') });
describe('structured model output', () => {
  it('carries the explicit operation and distinguishes a format retry from a new action', async () => {
    const run = vi
      .fn()
      .mockResolvedValueOnce({ answer: 'broken', trace })
      .mockResolvedValueOnce({ answer: '{"answer":"ok"}', trace });
    await structured(
      { id: 'codex', run },
      'STALKER',
      '',
      {},
      undefined,
      'medium',
      decode,
      'corpus-analysis',
    );
    expect(
      run.mock.calls.map(([request]) => [request.operation, request['format-attempt']]),
    ).toEqual([
      ['corpus-analysis', 1],
      ['corpus-analysis', 2],
    ]);
    const plain = vi.fn().mockResolvedValue({ answer: '{"answer":"ok"}', trace });
    await structured({ id: 'codex', run: plain }, 'STALKER', '', {}, undefined, 'medium', decode);
    expect(plain.mock.calls[0]![0].operation).toBe('generation');
  });
  it('repairs invalid field types once with precise feedback', async () => {
    const run = vi
      .fn()
      .mockResolvedValueOnce({ answer: '{"answer":{}}', trace })
      .mockResolvedValueOnce({ answer: '{"answer":"Исправленный ответ"}', trace });
    const result = await structured(
      { id: 'codex', run },
      'PERSON',
      'Return {answer:string}.',
      {},
      undefined,
      'high',
      decode,
    );
    expect(result.data.answer).toBe('Исправленный ответ');
    expect(run).toHaveBeenCalledTimes(2);
    expect(run.mock.calls[1]?.[0]['system-prompt']).toContain('answer');
    expect(run.mock.calls[1]?.[0]['system-prompt']).toContain('previous response');
  });
  it('stops after two invalid responses and identifies the stage and field', async () => {
    const run = vi.fn().mockResolvedValue({ answer: '{"answer":null}', trace });
    await expect(
      structured({ id: 'codex', run }, 'PERSON', '', {}, undefined, 'high', decode),
    ).rejects.toThrow(/PERSON.*answer/);
    expect(run).toHaveBeenCalledTimes(2);
  });
  it('repairs malformed JSON but does not retry CLI failures or cancellation', async () => {
    const run = vi
      .fn()
      .mockResolvedValueOnce({ answer: 'broken', trace })
      .mockResolvedValueOnce({ answer: '{"answer":"ok"}', trace });
    await expect(
      structured({ id: 'codex', run }, 'PERSON', '', {}, undefined, 'high', decode),
    ).resolves.toMatchObject({ data: { answer: 'ok' } });
    const unavailable: IntelligenceProvider = {
      id: 'codex',
      run: vi.fn().mockRejectedValue(new Error('CLI login required')),
    };
    await expect(structured(unavailable, 'PERSON', '', {})).rejects.toThrow('CLI login required');
    expect(unavailable.run).toHaveBeenCalledTimes(1);
    const controller = new AbortController();
    const cancelled = vi.fn().mockImplementation(async () => {
      controller.abort();
      return { answer: 'broken', trace };
    });
    await expect(
      structured({ id: 'codex', run: cancelled }, 'PERSON', '', {}, controller.signal),
    ).rejects.toThrow();
    expect(cancelled).toHaveBeenCalledTimes(1);
  });
});
