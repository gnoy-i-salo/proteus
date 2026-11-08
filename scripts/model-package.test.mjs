import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { mkdtemp, rm, writeFile, readFile, readdir, mkdir } from 'node:fs/promises';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { prepareModelFile, verifyModelFile } from './model-package.mjs';
const bytes = Buffer.from('точный synthetic model package\r\n');
const pin = {
  bytes: bytes.length,
  sha256: createHash('sha256').update(bytes).digest('hex'),
  url: 'https://example.invalid/pinned',
};
async function fixture(run) {
  const root = await mkdtemp(join(tmpdir(), 'proteus-model-package-'));
  try {
    await run(root, join(root, 'tokenizer.json'));
  } finally {
    await rm(root, { recursive: true, force: true });
  }
}
test('verified cached model is used without network or mutation', () =>
  fixture(async (_root, path) => {
    await writeFile(path, bytes);
    await prepareModelFile(path, pin, () => {
      throw new Error('network forbidden');
    });
    assert.deepEqual(await readFile(path), bytes);
  }));
test('wrong size, digest and directories fail without replacing existing files', () =>
  fixture(async (root, path) => {
    for (const wrong of [
      bytes.subarray(1),
      Buffer.alloc(bytes.length),
      Buffer.concat([bytes, bytes]),
    ]) {
      await writeFile(path, wrong);
      await assert.rejects(verifyModelFile(path, pin));
      await assert.rejects(
        prepareModelFile(path, pin, () => {
          throw new Error('network forbidden');
        }),
      );
      assert.deepEqual(await readFile(path), wrong);
    }
    await assert.rejects(verifyModelFile(root, pin));
  }));
test('download verifies before atomic installation and supports nested cache', () =>
  fixture(async (root) => {
    const path = join(root, 'nested', 'tokenizer.json');
    let calls = 0;
    await prepareModelFile(path, pin, async (url) => {
      assert.equal(url, pin.url);
      calls++;
      return new Response(bytes);
    });
    assert.equal(calls, 1);
    assert.deepEqual(await readFile(path), bytes);
    assert.deepEqual(await readdir(join(root, 'nested')), ['tokenizer.json']);
  }));
test('HTTP errors, interrupted, oversized and corrupted downloads never leave a model', () =>
  fixture(async (root, path) => {
    const responses = [
      () => new Response('', { status: 503 }),
      () => new Response(Buffer.alloc(bytes.length)),
      () => new Response(Buffer.concat([bytes, bytes])),
      () =>
        new Response(
          new ReadableStream({
            start(controller) {
              controller.enqueue(bytes.subarray(0, 3));
              controller.error(new Error('interrupted'));
            },
          }),
        ),
    ];
    for (const response of responses) {
      let calls = 0;
      await assert.rejects(
        prepareModelFile(path, pin, async () => {
          calls++;
          return response();
        }),
      );
      assert.equal(calls, 1);
      assert.deepEqual(await readdir(root), []);
    }
  }));
test('non-file destination is preserved', () =>
  fixture(async (_root, path) => {
    await mkdir(path);
    await assert.rejects(prepareModelFile(path, pin));
  }));
