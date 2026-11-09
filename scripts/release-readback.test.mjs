import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, writeFileSync, readFileSync, cpSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, delimiter } from 'node:path';
import { spawnSync } from 'node:child_process';
import { hashArtifact, releasePlatforms } from './release-provenance.mjs';

const workflow = readFileSync('.github/workflows/release.yml', 'utf8');
const step = workflow.split('      - name: Verify uploaded draft bytes and source\n')[1];
assert(step, 'missing uploaded artifact verification');
const command = step
  .split('        run: |\n')[1]
  .split('\n')
  .filter((line) => line.startsWith('          '))
  .map((line) => line.slice(10))
  .join('\n');
const tag = 'v0.4.0';
const sourceSha = 'a'.repeat(40);

function fixture(t) {
  const directory = mkdtempSync(join(tmpdir(), 'proteus-release-readback-'));
  t.after(() => rmSync(directory, { recursive: true, force: true }));
  for (const name of ['bin', 'release-assets', 'remote', 'download'])
    mkdirSync(join(directory, name));
  const assets = releasePlatforms(tag).map((item) => item.asset);
  for (const name of assets)
    writeFileSync(join(directory, 'release-assets', name), `synthetic ${name}`);
  writeFileSync(
    join(directory, 'release-assets/BUILD-MANIFEST.json'),
    JSON.stringify({ sourceSha, tag, assets }),
  );
  const sums = [...assets, 'BUILD-MANIFEST.json']
    .map((name) => `${hashArtifact(join(directory, 'release-assets', name)).sha256}  ${name}\n`)
    .join('');
  writeFileSync(join(directory, 'release-assets/SHA256SUMS.txt'), sums);
  cpSync(join(directory, 'release-assets'), join(directory, 'remote'), { recursive: true });
  const executable = (name, body) =>
    writeFileSync(join(directory, 'bin', name), `#!/usr/bin/env node\n${body}\n`, { mode: 0o755 });
  executable('mktemp', `process.stdout.write(process.env.READBACK_DIRECTORY + '/download\\n');`);
  executable(
    'gh',
    `
    const fs = require('node:fs');
    const args = process.argv.slice(2);
    if (JSON.stringify(args) === JSON.stringify(['release','download','${tag}','--dir',process.env.READBACK_DIRECTORY+'/download'])) {
      if (process.env.READBACK_DOWNLOAD_FAILURE === 'true') process.exit(1);
      fs.cpSync(process.env.READBACK_DIRECTORY+'/remote',args[4],{recursive:true});
    } else if (JSON.stringify(args) === JSON.stringify(['release','view','${tag}','--json','isDraft','--jq','.isDraft'])) {
      process.stdout.write(process.env.READBACK_DRAFT + '\\n');
    } else process.exit(2);
  `,
  );
  executable(
    'git',
    `
    const args = process.argv.slice(2);
    if (JSON.stringify(args) === JSON.stringify(['fetch','--no-tags','--depth=1','origin','refs/tags/${tag}'])) process.exit(0);
    if (JSON.stringify(args) === JSON.stringify(['rev-parse','--verify','FETCH_HEAD^{commit}'])) process.stdout.write(process.env.READBACK_TAG_SHA + '\\n');
    else process.exit(2);
  `,
  );
  const run = (env = {}) =>
    spawnSync('bash', ['-e', '-o', 'pipefail', '-c', command], {
      cwd: directory,
      env: {
        ...process.env,
        PATH: join(directory, 'bin') + delimiter + process.env.PATH,
        READBACK_DIRECTORY: directory,
        READBACK_DRAFT: 'true',
        READBACK_TAG_SHA: sourceSha,
        SOURCE_SHA: sourceSha,
        RELEASE_TAG: tag,
        ...env,
      },
      encoding: 'utf8',
    });
  return { directory, assets, run };
}
test('actual readback shell accepts identical downloaded bytes, pinned tag and draft state', (t) => {
  const f = fixture(t);
  const result = f.run();
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /BUILD-MANIFEST.json: OK/);
});
test('actual readback shell rejects damaged or missing uploaded files and forged checksum documents', (t) => {
  for (const kind of ['installer', 'missing', 'checksums', 'manifest']) {
    const f = fixture(t);
    const file =
      kind === 'checksums'
        ? 'SHA256SUMS.txt'
        : kind === 'manifest'
          ? 'BUILD-MANIFEST.json'
          : f.assets[0];
    if (kind === 'missing') rmSync(join(f.directory, 'remote', file));
    else writeFileSync(join(f.directory, 'remote', file), 'changed remote bytes');
    assert.notEqual(f.run().status, 0, kind);
  }
});
test('actual readback shell rejects failed download, moved tag and published release', (t) => {
  for (const env of [
    { READBACK_DOWNLOAD_FAILURE: 'true' },
    { READBACK_TAG_SHA: 'b'.repeat(40) },
    { READBACK_DRAFT: 'false' },
  ]) {
    const f = fixture(t);
    assert.notEqual(f.run(env).status, 0, JSON.stringify(env));
  }
});
