import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { mkdtempSync, writeFileSync, rmSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import {
  hashArtifact,
  validateReleaseReadiness,
  validateSourceSha,
  verifyBuildReceipts,
} from './release-provenance.mjs';
const tag = 'v0.4.0';
const sourceSha = 'a'.repeat(40);
function issues() {
  return [
    ['tip', 17],
    ['gx9', 26],
    ['x1y', 17],
  ].flatMap(([epic, count]) =>
    Array.from({ length: count }, (_, i) => ({ id: `proteus-${epic}.${i + 1}`, status: 'closed' })),
  );
}
function build() {
  const targets = [
    ['macos-arm64', 'aarch64-apple-darwin', 'dmg'],
    ['macos-x64', 'x86_64-apple-darwin', 'dmg'],
    ['windows-x64', 'x86_64-pc-windows-msvc', 'exe'],
    ['linux-x64', 'x86_64-unknown-linux-gnu', 'deb'],
  ];
  const artifacts = new Map();
  const receipts = targets.map(([platform, target, extension]) => {
    const asset = `proteus-${tag}-${platform}.${extension}`;
    const bytes = Buffer.from(`synthetic installer ${platform}`);
    artifacts.set(asset, {
      sha256: createHash('sha256').update(bytes).digest('hex'),
      sizeBytes: bytes.length,
    });
    return {
      schemaVersion: 1,
      tag,
      sourceSha,
      platform,
      target,
      asset,
      sha256: createHash('sha256').update(bytes).digest('hex'),
      sizeBytes: bytes.length,
    };
  });
  return { artifacts, receipts };
}
test('release requires all three epic child acceptances while its own draft task remains open', () => {
  assert.doesNotThrow(() =>
    validateReleaseReadiness([...issues(), { id: 'proteus-x1y.18', status: 'in_progress' }]),
  );
  for (const { id } of issues()) {
    assert.throws(
      () => validateReleaseReadiness(issues().filter((issue) => issue.id !== id)),
      /acceptance|missing/i,
    );
    assert.throws(
      () =>
        validateReleaseReadiness(
          issues().map((issue) => (issue.id === id ? { ...issue, status: 'in_progress' } : issue)),
        ),
      /acceptance|closed/i,
    );
  }
  assert.throws(() => validateReleaseReadiness([...issues(), issues()[0]]), /duplicate/i);
});
test('source comes from the checked out commit and must equal the quality gate SHA', () => {
  assert.equal(validateSourceSha(sourceSha, sourceSha), sourceSha);
  for (const value of ['', 'v0.4.0', `${sourceSha}\n`, 'b'.repeat(40), null])
    assert.throws(() => validateSourceSha(value, sourceSha), /source|SHA/i);
  assert.throws(() => validateSourceSha(sourceSha, undefined), /source|SHA/i);
});
test('four matching build receipts produce deterministic provenance over actual installer bytes', () => {
  const { receipts, artifacts } = build();
  const result = verifyBuildReceipts(tag, sourceSha, [...receipts].reverse(), artifacts);
  assert.equal(result.schemaVersion, 1);
  assert.equal(result.sourceSha, sourceSha);
  assert.equal(result.tag, tag);
  assert.deepEqual(
    result.assets,
    receipts.map(({ platform, target, asset, sha256, sizeBytes }) => ({
      platform,
      target,
      asset,
      sha256,
      sizeBytes,
    })),
  );
});
for (const [name, patch] of [
  ['mixed commit', { sourceSha: 'b'.repeat(40) }],
  ['foreign tag', { tag: 'v0.3.0' }],
  ['wrong target', { target: 'x86_64-unknown-linux-gnu' }],
  ['renamed installer', { asset: '../foreign.dmg' }],
  ['bad checksum', { sha256: '0'.repeat(64) }],
  ['wrong size', { sizeBytes: 0 }],
  ['unsupported receipt', { schemaVersion: 2 }],
  ['conflicting metadata', { sourceRevision: sourceSha }],
])
  test(`rejects ${name} before assembling a draft`, () => {
    const { receipts, artifacts } = build();
    receipts[0] = { ...receipts[0], ...patch };
    assert.throws(() => verifyBuildReceipts(tag, sourceSha, receipts, artifacts));
  });
test('missing, duplicate and extra platforms or changed bytes cannot satisfy four builds', () => {
  const { receipts, artifacts } = build();
  assert.throws(() => verifyBuildReceipts(tag, sourceSha, receipts.slice(1), artifacts));
  assert.throws(() => verifyBuildReceipts(tag, sourceSha, [...receipts, receipts[0]], artifacts));
  assert.throws(() =>
    verifyBuildReceipts(
      tag,
      sourceSha,
      [receipts[0], receipts[0], ...receipts.slice(2)],
      artifacts,
    ),
  );
  artifacts.set(receipts[0].asset, {
    sha256: createHash('sha256').update('changed installer').digest('hex'),
    sizeBytes: 17,
  });
  assert.throws(() => verifyBuildReceipts(tag, sourceSha, receipts, artifacts));
});

test('installer digest covers multiple buffers and rejects empty files and directories', (t) => {
  const directory = mkdtempSync(join(tmpdir(), 'proteus-installer-digest-'));
  t.after(() => rmSync(directory, { recursive: true, force: true }));
  const path = join(directory, 'package é.bin');
  const bytes = Buffer.concat([Buffer.alloc(130001, 0xa5), Buffer.from('exact é👩🏽‍🔬\r\n')]);
  writeFileSync(path, bytes);
  assert.deepEqual(hashArtifact(path), {
    sha256: createHash('sha256').update(bytes).digest('hex'),
    sizeBytes: bytes.length,
  });
  writeFileSync(path, '');
  assert.throws(() => hashArtifact(path), /size/);
  assert.throws(() => hashArtifact(directory), /regular/);
});
