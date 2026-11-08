import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const ciWorkflow = readFileSync('.github/workflows/ci.yml', 'utf8');
const releaseWorkflow = readFileSync('.github/workflows/release.yml', 'utf8');

const CACHE_ACTION = 'Swatinem/rust-cache@v2';
const CI_CACHE_STEP = `      - uses: ${CACHE_ACTION}
        with:
          workspaces: apps/desktop/src-tauri
          save-if: \${{ github.event_name != 'pull_request' }}
`;
const SAVE_IF_LINE = "          save-if: ${{ github.event_name != 'pull_request' }}";
const PR_SAVE_IF_LINE = "          save-if: ${{ github.event_name == 'pull_request' }}";
const RELEASE_CACHE_STEP = `      - uses: ${CACHE_ACTION}
        with:
          workspaces: apps/desktop/src-tauri
          key: \${{ matrix.target }}
          save-if: \${{ github.event_name != 'pull_request' }}
`;
const RUSTUP_INSTALL =
  'rustup toolchain install stable --profile minimal --component rustfmt,clippy';

function jobSection(text, name, followingJobs) {
  const start = text.indexOf(`  ${name}:\n`);
  assert.notEqual(start, -1, `job ${name} not found`);
  let end = text.length;
  for (const next of followingJobs) {
    const index = text.indexOf(`  ${next}:\n`, start + 1);
    if (index !== -1) end = Math.min(end, index);
  }
  return text.slice(start, end);
}

function cacheStepBlock(job) {
  const at = job.indexOf(`- uses: ${CACHE_ACTION}`);
  assert.notEqual(at, -1, `missing ${CACHE_ACTION} step`);
  const end = job.indexOf('\n      - ', at);
  return job.slice(at, end === -1 ? job.length : end);
}

function verifyCacheStep(job, { targetKey }) {
  const lines = cacheStepBlock(job)
    .split('\n')
    .map((line) => line.trimEnd());
  assert.ok(lines.includes(`- uses: ${CACHE_ACTION}`), 'cache step must pin the action');
  assert.ok(
    lines.includes('          workspaces: apps/desktop/src-tauri'),
    'cache must map the Rust workspace',
  );
  assert.ok(lines.includes(SAVE_IF_LINE), 'cache must only save outside pull_request');
  if (targetKey) {
    assert.ok(
      lines.includes('          key: ${{ matrix.target }}'),
      'cache must isolate each Rust target',
    );
  }
  const installAt = job.indexOf(RUSTUP_INSTALL);
  assert.notEqual(installAt, -1, 'toolchain install must stay before the cache step');
  assert.ok(
    job.indexOf(`- uses: ${CACHE_ACTION}`) > installAt,
    'cache restores only after the toolchain is installed',
  );
  assert.ok(
    job.indexOf(`- uses: ${CACHE_ACTION}`) < job.indexOf('pnpm check:rust'),
    'cache must precede the first Rust gate',
  );
  assert.doesNotMatch(job, /cache-hit/, 'Rust and native gates must not be skipped on a cache hit');
}

function verifyNativeGates(job) {
  assert.match(job, /^ {4}timeout-minutes: 180$/m, 'native timeout budget must not be weakened');
  assert.match(job, /^ {8}run: pnpm prepare:tdlib$/m);
  assert.match(job, /^ {8}run: pnpm prepare:models$/m);
  assert.match(job, /^ {8}run: pnpm prepare:embedding-model$/m);
  assert.match(job, /^ {6}- run: pnpm build$/m);
  assert.match(job, /^ {6}- run: pnpm check:rust$/m);
  assert.match(job, /actions\/upload-artifact@v7/);
}

function verifyCiNative(text) {
  assert.match(
    text,
    /^permissions:\n {2}contents: read\nconcurrency:/m,
    'cache needs no extra permissions',
  );
  const quality = jobSection(text, 'quality', ['native', 'pages-build', 'pages-deploy']);
  const native = jobSection(text, 'native', ['pages-build', 'pages-deploy']);
  const pagesBuild = jobSection(text, 'pages-build', ['pages-deploy']);
  const pagesDeploy = jobSection(text, 'pages-deploy', []);
  assert.doesNotMatch(
    quality + pagesBuild + pagesDeploy,
    /rust-cache/,
    'cache belongs to the native matrix only',
  );
  verifyCacheStep(native, { targetKey: false });
  verifyNativeGates(native);
  assert.match(native, /^ {6}- run: pnpm build:desktop$/m);
}

function verifyReleaseBuild(text) {
  assert.match(
    text,
    /^permissions:\n {2}contents: read\nconcurrency:/m,
    'cache needs no extra permissions',
  );
  const quality = jobSection(text, 'quality', ['build', 'publish']);
  const build = jobSection(text, 'build', ['publish']);
  const publish = jobSection(text, 'publish', []);
  assert.doesNotMatch(quality + publish, /rust-cache/, 'cache belongs to the build matrix only');
  verifyCacheStep(build, { targetKey: true });
  verifyNativeGates(build);
  assert.match(build, /^ {10}ref: \$\{\{ needs\.quality\.outputs\.sourceSha \}\}$/m);
  assert.match(build, /^ {6}SOURCE_SHA: \$\{\{ needs\.quality\.outputs\.sourceSha \}\}$/m);
  assert.match(build, /rustup target add \$\{\{ matrix\.target \}\}/);
  assert.match(build, /-- --locked$/m, 'release build must stay locked to Cargo.lock');
  assert.match(build, /node scripts\/release\.mjs collect/);
}

test('CI native matrix restores the Rust dependency cache without touching required gates', () =>
  verifyCiNative(ciWorkflow));

test('Release four-platform build restores the Rust cache per target and preserves one-SHA gates', () =>
  verifyReleaseBuild(releaseWorkflow));

test('CI fixtures reject missing cache, wrong workspace, PR saves, reordered setup and dropped gates', () => {
  const valid = ciWorkflow;
  const qualityWithCache = `${CI_CACHE_STEP}      - run: pnpm check\n`;
  assert.throws(() => verifyCiNative(valid.replace(`- uses: ${CACHE_ACTION}\n`, '')));
  assert.throws(() =>
    verifyCiNative(valid.replace('workspaces: apps/desktop/src-tauri', 'workspaces: .')),
  );
  assert.throws(() => verifyCiNative(valid.replace(SAVE_IF_LINE, PR_SAVE_IF_LINE)));
  assert.throws(() => verifyCiNative(valid.replace(`${SAVE_IF_LINE}\n`, '')));
  assert.throws(() =>
    verifyCiNative(
      valid.replace(
        `      - name: Install Rust tools\n        run: ${RUSTUP_INSTALL}\n`,
        `${CI_CACHE_STEP}      - name: Install Rust tools\n        run: ${RUSTUP_INSTALL}\n`,
      ),
    ),
  );
  assert.throws(() => verifyCiNative(valid.replace('      - run: pnpm check:rust\n', '')));
  assert.throws(() => verifyCiNative(valid.replace('        run: pnpm prepare:tdlib\n', '')));
  assert.throws(() => verifyCiNative(valid.replace('      - run: pnpm build:desktop\n', '')));
  assert.throws(() => verifyCiNative(valid.replace('      - run: pnpm check\n', qualityWithCache)));
});

test('Release fixtures reject shared targets, PR saves, unpinned source and weakened verification', () => {
  const valid = releaseWorkflow;
  assert.throws(() =>
    verifyReleaseBuild(valid.replace('          key: ${{ matrix.target }}\n', '')),
  );
  assert.throws(() =>
    verifyReleaseBuild(valid.replace('key: ${{ matrix.target }}', 'key: ${{ matrix.os }}')),
  );
  assert.throws(() => verifyReleaseBuild(valid.replace(SAVE_IF_LINE, PR_SAVE_IF_LINE)));
  assert.throws(() =>
    verifyReleaseBuild(
      valid.replace('ref: ${{ needs.quality.outputs.sourceSha }}', 'ref: ${{ github.ref }}'),
    ),
  );
  assert.throws(() => verifyReleaseBuild(valid.replace('-- --locked', '')));
  assert.throws(() => verifyReleaseBuild(valid.replace('      - run: pnpm check:rust\n', '')));
  assert.throws(() =>
    verifyReleaseBuild(valid.replace('        run: pnpm prepare:embedding-model\n', '')),
  );
  assert.throws(() =>
    verifyReleaseBuild(
      valid.replace(
        `      - run: ${RUSTUP_INSTALL}\n`,
        `${RELEASE_CACHE_STEP}      - run: ${RUSTUP_INSTALL}\n`,
      ),
    ),
  );
});

test('cache hits cannot skip verification in either native matrix', () => {
  for (const [workflow, verify] of [
    [ciWorkflow, verifyCiNative],
    [releaseWorkflow, verifyReleaseBuild],
  ]) {
    const changed = workflow.replace(
      '      - run: pnpm check:rust\n',
      "      - run: pnpm check:rust\n        if: steps.rustCache.outputs.cache-hit != 'true'\n",
    );
    assert.notEqual(changed, workflow, 'fixture must change a real Rust gate');
    assert.throws(() => verify(changed), /must not be skipped/);
  }
});
