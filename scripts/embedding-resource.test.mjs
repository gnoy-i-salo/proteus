import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
const read = (path) => readFileSync(path, 'utf8');
function verifyWiring(manifest, workflow, runtime, tokenizer) {
  assert.match(manifest, /candle-core = \{ version = "=0\.11\.1", default-features = false \}/);
  assert.match(manifest, /candle-nn = \{ version = "=0\.11\.1", default-features = false \}/);
  assert.match(
    manifest,
    /candle-transformers = \{ version = "=0\.11\.1", default-features = false \}/,
  );
  assert.equal(runtime.revision, tokenizer.revision);
  assert.equal(runtime.runtimeVersion, 'candle:0.11.1:cpuF32:maskedMeanF64L2:1');
  const native = workflow.split('      - run: pnpm check:rust')[0];
  assert.match(native, /run: pnpm prepare:embedding-model/);
}
const manifest = read('apps/desktop/src-tauri/Cargo.toml');
const runtime = JSON.parse(read('scripts/e5-embedding-package.json'));
const tokenizer = JSON.parse(read('scripts/e5-package.json'));
test('native test matrices prepare the exact CPU model before running real inference', () => {
  for (const workflow of ['.github/workflows/ci.yml', '.github/workflows/release.yml'])
    verifyWiring(manifest, read(workflow), runtime, tokenizer);
  const scripts = JSON.parse(read('package.json')).scripts;
  assert.equal(scripts['prepare:embedding-model'], 'node scripts/prepare-embedding-model.mjs');
});
test('missing preparation, changed runtime and mixed model revisions are rejected', () => {
  const valid = 'run: pnpm prepare:embedding-model\n      - run: pnpm check:rust';
  verifyWiring(manifest, valid, runtime, tokenizer);
  assert.throws(() => verifyWiring(manifest, '', runtime, tokenizer));
  assert.throws(() => verifyWiring(manifest.replace('=0.11.1', '0.11'), valid, runtime, tokenizer));
  assert.throws(() =>
    verifyWiring(manifest, valid, { ...runtime, revision: 'different' }, tokenizer),
  );
  assert.throws(() =>
    verifyWiring(manifest, valid, { ...runtime, runtimeVersion: 'different' }, tokenizer),
  );
});
