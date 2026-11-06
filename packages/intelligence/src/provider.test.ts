import { describe, expect, it } from 'vitest';
import { createUnconfiguredProvider, IntelligenceRunError } from './index';
describe('unconfigured providers', () => {
  it.each(['codex', 'kimi'] as const)(
    'fails clearly instead of fabricating an answer for %s',
    async (id) => {
      await expect(
        createUnconfiguredProvider(id).run({
          task: 'example',
          'system-prompt': '',
          effort: 'medium',
        }),
      ).rejects.toThrow('not configured');
    },
  );
});
it('retains an independent copy of published trace data and the failure cause', () => {
  const block = { id: 'r', kind: 'summary' as const, text: 'Public snapshot' };
  const cause = new Error('Provider stopped');
  const failure = new IntelligenceRunError(
    'Interrupted',
    {
      provider: 'codex',
      'reasoning-ms': 40,
      'wall-time-ms': 40,
      'active-reasoning-ms': null,
      'terminal-ms': 0,
      summary: 'Partial',
      reasoning: [block],
    },
    { cause },
  );
  block.text = 'Later mutation';
  expect(failure).toBeInstanceOf(Error);
  expect(failure.cause).toBe(cause);
  expect(failure.trace.reasoning?.[0]?.text).toBe('Public snapshot');
  expect(failure.trace['active-reasoning-ms']).toBeNull();
});
