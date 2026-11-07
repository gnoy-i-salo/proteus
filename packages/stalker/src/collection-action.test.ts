import { describe, expect, it } from 'vitest';
import { parseCollectionAction } from './collection-action';

describe('source action allowlist', () => {
  it.each([
    { action: 'send', text: 'hello' },
    { action: 'collect', cursor: null, limit: 21 },
    { action: 'read', documentIds: [] },
    { action: 'search', query: 'x', limit: 1, url: 'https://other.example' },
    { action: 'finish', reason: 'done', token: 'secret' },
  ])('rejects malformed or unlisted action %j', (value) => {
    expect(() => parseCollectionAction(value)).toThrow(/action/i);
  });
});
