import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
const read = (path) => readFileSync(path, 'utf8');
function verifyWiring(config, build, workflow) {
  assert.equal(
    config.bundle.resources['../../../.cache/models/e5-tokenizer.json'],
    'models/e5-tokenizer.json',
  );
  assert.equal(config.bundle.resources['third-party-notices.txt'], 'third-party-notices.txt');
  assert.match(build, /tokenizer_package::read\(&tokenizer_path, &pin\)/);
  assert.match(build, /cargo:rerun-if-changed=.*e5-package.json/);
  const native = workflow.split('      - run: pnpm check:rust')[0];
  assert.match(native, /run: pnpm prepare:models/);
}
const config = JSON.parse(read('apps/desktop/src-tauri/tauri.conf.json'));
const build = read('apps/desktop/src-tauri/build.rs');
test('native bundles include the pinned resource and verify it before compiling', () => {
  for (const workflow of ['.github/workflows/ci.yml', '.github/workflows/release.yml'])
    verifyWiring(config, build, read(workflow));
});
test('missing resources, skipped verification and skipped preparation cannot satisfy the package gate', () => {
  const valid = {
    bundle: {
      resources: {
        '../../../.cache/models/e5-tokenizer.json': 'models/e5-tokenizer.json',
        'third-party-notices.txt': 'third-party-notices.txt',
      },
    },
  };
  const check =
    'tokenizer_package::read(&tokenizer_path, &pin); cargo:rerun-if-changed=scripts/e5-package.json';
  const workflow = 'run: pnpm prepare:models\n      - run: pnpm check:rust';
  verifyWiring(valid, check, workflow);
  assert.throws(() => verifyWiring({ bundle: { resources: {} } }, check, workflow));
  assert.throws(() => verifyWiring(valid, '', workflow));
  assert.throws(() => verifyWiring(valid, check, '      - run: pnpm check:rust'));
});
