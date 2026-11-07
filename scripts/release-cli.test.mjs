import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, writeFileSync, readFileSync, rmSync, existsSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { execFileSync, spawnSync } from 'node:child_process';
import { releasePlatforms, hashArtifact } from './release-provenance.mjs';
const script = resolve('scripts/release.mjs');
// Only read the current checkout's object identity; tests never modify its Git state.
const gitDir = execFileSync('git', ['rev-parse', '--absolute-git-dir'], {
  encoding: 'utf8',
}).trim();
const sourceSha = execFileSync('git', ['rev-parse', '--verify', 'HEAD^{commit}'], {
  encoding: 'utf8',
}).trim();
const tag = 'v0.4.0';
function fixture(t) {
  const directory = mkdtempSync(join(tmpdir(), 'proteus-release-proof-'));
  t.after(() => rmSync(directory, { recursive: true, force: true }));
  const write = (path, value) => {
    const file = join(directory, path);
    mkdirSync(resolve(file, '..'), { recursive: true });
    writeFileSync(file, value);
  };
  for (const file of [
    'package.json',
    'apps/desktop/package.json',
    'apps/desktop/src-tauri/tauri.conf.json',
  ])
    write(file, JSON.stringify({ version: '0.4.0' }));
  write('apps/desktop/src-tauri/Cargo.toml', '[package]\nname = "proteus"\nversion = "0.4.0"\n');
  write(
    'apps/desktop/src-tauri/Cargo.lock',
    'version = 4\n\n[[package]]\nname = "proteus"\nversion = "0.4.0"\n',
  );
  const issues = [
    ['tip', 17],
    ['gx9', 26],
    ['x1y', 17],
  ].flatMap(([epic, count]) =>
    Array.from({ length: count }, (_, i) => ({ id: `proteus-${epic}.${i + 1}`, status: 'closed' })),
  );
  const saveIssues = () =>
    write('.beads/issues.jsonl', issues.map((issue) => JSON.stringify(issue)).join('\n') + '\n');
  saveIssues();
  const run = (mode, env = {}) =>
    spawnSync(process.execPath, [script, ...(mode ? [mode] : [])], {
      cwd: directory,
      env: { ...process.env, GIT_DIR: gitDir, RELEASE_TAG: tag, SOURCE_SHA: sourceSha, ...env },
      encoding: 'utf8',
    });
  const collect = (platform, env = {}) => {
    write(
      `apps/desktop/src-tauri/target/${platform.target}/release/bundle/installer.${platform.extension}`,
      `synthetic installer ${platform.platform}`,
    );
    return run('collect', {
      TARGET: platform.target,
      PLATFORM: platform.platform,
      EXTENSION: platform.extension,
      ...env,
    });
  };
  return { directory, write, issues, saveIssues, run, collect };
}
test('release CLI refuses unaccepted final quality even when versions match', (t) => {
  const f = fixture(t);
  f.issues.find((issue) => issue.id === 'proteus-gx9.26').status = 'in_progress';
  f.saveIssues();
  const result = f.run();
  assert.notEqual(result.status, 0, 'unaccepted gx9.26 must block the release');
  assert.match(result.stderr, /proteus-gx9.26/);
});
test('collection verifies the checkout SHA before writing any installer', (t) => {
  const f = fixture(t);
  const platform = releasePlatforms(tag)[0];
  const result = f.collect(platform, { SOURCE_SHA: 'b'.repeat(40) });
  assert.notEqual(result.status, 0, 'foreign source SHA must not produce a collected artifact');
  assert.equal(existsSync(join(f.directory, 'release-assets', platform.asset)), false);
});
test('quality captures the actual checkout SHA and collection records all four builds', (t) => {
  const f = fixture(t);
  const output = join(f.directory, 'job-output');
  const source = f.run('source', { SOURCE_SHA: '', GITHUB_OUTPUT: output });
  assert.equal(source.status, 0, source.stderr);
  assert.equal(readFileSync(output, 'utf8'), `sourceSha=${sourceSha}\n`);
  for (const platform of releasePlatforms(tag)) {
    const result = f.collect(platform);
    assert.equal(result.status, 0, result.stderr);
    const receipt = JSON.parse(
      readFileSync(join(f.directory, 'release-assets', `${platform.asset}.source.json`), 'utf8'),
    );
    assert.deepEqual(receipt, {
      schemaVersion: 1,
      tag,
      sourceSha,
      platform: platform.platform,
      target: platform.target,
      asset: platform.asset,
      ...hashArtifact(join(f.directory, 'release-assets', platform.asset)),
    });
  }
  const result = f.run('checksums');
  assert.equal(result.status, 0, result.stderr);
  const manifest = JSON.parse(
    readFileSync(join(f.directory, 'release-assets/BUILD-MANIFEST.json'), 'utf8'),
  );
  assert.equal(manifest.sourceSha, sourceSha);
  assert.equal(manifest.assets.length, 4);
  const lines = readFileSync(join(f.directory, 'release-assets/SHA256SUMS.txt'), 'utf8')
    .trim()
    .split('\n');
  assert.equal(lines.length, 5);
  for (const name of [
    ...releasePlatforms(tag).map((platform) => platform.asset),
    'BUILD-MANIFEST.json',
  ])
    assert(
      lines.includes(`${hashArtifact(join(f.directory, 'release-assets', name)).sha256}  ${name}`),
    );
});
test('checksums cannot bless foreign receipts, missing receipts or changed installers', (t) => {
  for (const scenario of ['foreign', 'missing', 'modified']) {
    const f = fixture(t);
    const platforms = releasePlatforms(tag);
    // Synthetic complete build output isolates the verifier from the collection command.
    for (const platform of platforms) {
      f.write(`release-assets/${platform.asset}`, `synthetic installer ${platform.platform}`);
      f.write(
        `release-assets/${platform.asset}.source.json`,
        JSON.stringify({
          schemaVersion: 1,
          tag,
          sourceSha,
          platform: platform.platform,
          target: platform.target,
          asset: platform.asset,
          ...hashArtifact(join(f.directory, 'release-assets', platform.asset)),
        }),
      );
    }
    const receiptPath = `release-assets/${platforms[0].asset}.source.json`;
    if (scenario === 'foreign') {
      const receipt = JSON.parse(readFileSync(join(f.directory, receiptPath), 'utf8'));
      f.write(receiptPath, JSON.stringify({ ...receipt, sourceSha: 'b'.repeat(40) }));
    } else if (scenario === 'missing') rmSync(join(f.directory, receiptPath));
    else f.write(`release-assets/${platforms[0].asset}`, 'tampered');
    const result = f.run('checksums');
    assert.notEqual(result.status, 0, scenario);
    assert.equal(existsSync(join(f.directory, 'release-assets/SHA256SUMS.txt')), false, scenario);
  }
});

test('release CLI rejects native lockfile version drift before any build is collected', (t) => {
  const f = fixture(t);
  f.write(
    'apps/desktop/src-tauri/Cargo.lock',
    'version = 4\n\n[[package]]\nname = "proteus"\nversion = "0.3.0"\n',
  );
  const result = f.run();
  assert.notEqual(result.status, 0, 'Cargo.lock must match the release version too');
});

test('collection refuses unsupported platform inputs before resolving bundle paths', (t) => {
  for (const env of [{ TARGET: '../foreign' }, { PLATFORM: 'unknown' }, { EXTENSION: 'txt' }]) {
    const f = fixture(t);
    const result = f.collect(releasePlatforms(tag)[0], env);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /Unsupported release platform/);
    assert.equal(existsSync(join(f.directory, 'release-assets')), false);
  }
});
