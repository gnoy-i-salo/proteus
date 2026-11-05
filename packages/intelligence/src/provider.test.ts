import { describe, expect, it } from 'vitest';
import { createUnconfiguredProvider } from './index';
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
