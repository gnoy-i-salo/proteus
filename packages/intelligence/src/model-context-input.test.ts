import { describe, expect, it } from 'vitest';
import fixtures from './fixtures/model-context-input.json';
import { modelContextInput } from './model-context-input';

describe('exact model context input identity', () => {
  for (const fixture of fixtures) {
    it(`matches the native shared UTF8 fixture: ${fixture.id}`, async () => {
      expect(await modelContextInput(fixture)).toEqual({
        inputBytes: fixture.inputBytes,
        inputSha256: fixture.inputSha256,
      });
    });
  }
  it('captures both strings before asynchronous hashing and never trims or normalizes', async () => {
    const request = { systemPrompt: ' é\r\n ', task: '😀' };
    const pending = modelContextInput(request);
    request.task = 'changed';
    expect(await pending).toEqual(await modelContextInput({ systemPrompt: ' é\r\n ', task: '😀' }));
    expect(await pending).not.toEqual(
      await modelContextInput({ systemPrompt: ' é\n ', task: '😀' }),
    );
  });
  it('counts escaped framing bytes and accepts the exact 4 MiB boundary', async () => {
    const task = 'a'.repeat(4 * 1024 * 1024 - 7);
    expect((await modelContextInput({ systemPrompt: '', task })).inputBytes).toBe(4 * 1024 * 1024);
    await expect(modelContextInput({ systemPrompt: '', task: task + 'a' })).rejects.toThrow();
    await expect(
      modelContextInput({ systemPrompt: '', task: '\u0000'.repeat(700000) }),
    ).rejects.toThrow();
  });
  it('rejects lone UTF16 surrogates instead of allowing native and browser text to differ', async () => {
    for (const invalid of ['\ud800', '\udfff', 'a\ud800b']) {
      await expect(modelContextInput({ systemPrompt: invalid, task: '' })).rejects.toThrow();
      await expect(modelContextInput({ systemPrompt: '', task: invalid })).rejects.toThrow();
    }
  });
});
