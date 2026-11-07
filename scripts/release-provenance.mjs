import { createHash } from 'node:crypto';
import { openSync, closeSync, readSync, fstatSync, lstatSync } from 'node:fs';
const platforms = [
  { platform: 'macos-arm64', target: 'aarch64-apple-darwin', extension: 'dmg' },
  { platform: 'macos-x64', target: 'x86_64-apple-darwin', extension: 'dmg' },
  { platform: 'windows-x64', target: 'x86_64-pc-windows-msvc', extension: 'exe' },
  { platform: 'linux-x64', target: 'x86_64-unknown-linux-gnu', extension: 'deb' },
];
export function validateRelease(tag, versions) {
  if (!/^v\d+\.\d+\.\d+$/u.test(tag)) throw new Error('Expected a stable version tag: vX.Y.Z');
  const version = tag.slice(1);
  if (!versions.length || versions.some((value) => value !== version))
    throw new Error('Release tag must match all application versions');
  return version;
}
export function releasePlatforms(tag) {
  validateRelease(tag, [tag.slice(1)]);
  return platforms.map((platform) => ({
    ...platform,
    asset: `proteus-${tag}-${platform.platform}.${platform.extension}`,
  }));
}
export function requiredAssets(tag, files) {
  const expected = releasePlatforms(tag).map((platform) => platform.asset);
  for (const name of expected)
    if (!files.includes(name)) throw new Error(`Missing release asset: ${name}`);
  return expected;
}
export function validateReleaseReadiness(issues) {
  if (!Array.isArray(issues)) throw new Error('Missing release acceptance snapshot');
  const statuses = new Map();
  for (const issue of issues) {
    if (!issue || typeof issue.id !== 'string' || typeof issue.status !== 'string')
      throw new Error('Invalid release acceptance snapshot');
    if (statuses.has(issue.id)) throw new Error(`Duplicate issue: ${issue.id}`);
    statuses.set(issue.id, issue.status);
  }
  for (const [epic, count] of [
    ['tip', 17],
    ['gx9', 26],
    ['x1y', 17],
  ]) {
    for (let number = 1; number <= count; number++) {
      const id = `proteus-${epic}.${number}`;
      if (statuses.get(id) !== 'closed')
        throw new Error(`Release acceptance must be closed: ${id}`);
    }
  }
}
export function validateSourceSha(actual, expected) {
  if (
    typeof actual !== 'string' ||
    !/^(?:[a-f0-9]{40}|[a-f0-9]{64})$/u.test(actual) ||
    actual !== expected
  )
    throw new Error('Release source SHA does not match the verified quality checkout');
  return actual;
}
export function hashArtifact(path) {
  if (!lstatSync(path).isFile()) throw new Error('Installer must be a regular file');
  const fd = openSync(path, 'r');
  try {
    const size = fstatSync(fd).size;
    if (!Number.isSafeInteger(size) || size <= 0) throw new Error('Installer size is invalid');
    const hash = createHash('sha256');
    const buffer = Buffer.alloc(64 * 1024);
    let sizeBytes = 0;
    for (;;) {
      const count = readSync(fd, buffer, 0, buffer.length, null);
      if (!count) break;
      hash.update(buffer.subarray(0, count));
      sizeBytes += count;
    }
    if (sizeBytes !== size || fstatSync(fd).size !== size)
      throw new Error('Installer changed while hashing');
    return { sha256: hash.digest('hex'), sizeBytes };
  } finally {
    closeSync(fd);
  }
}
export function verifyBuildReceipts(tag, sourceSha, receipts, artifacts) {
  validateSourceSha(sourceSha, sourceSha);
  const expected = releasePlatforms(tag);
  if (
    !Array.isArray(receipts) ||
    receipts.length !== expected.length ||
    artifacts.size !== expected.length
  )
    throw new Error('Expected exactly four installer receipts and files');
  const keys = [
    'schemaVersion',
    'tag',
    'sourceSha',
    'platform',
    'target',
    'asset',
    'sha256',
    'sizeBytes',
  ]
    .sort()
    .join(',');
  const seen = new Set();
  for (const receipt of receipts) {
    if (!receipt || Object.keys(receipt).sort().join(',') !== keys || receipt.schemaVersion !== 1)
      throw new Error('Invalid build receipt schema');
    validateSourceSha(receipt.sourceSha, sourceSha);
    const platform = expected.find((item) => item.platform === receipt.platform);
    if (
      !platform ||
      seen.has(receipt.platform) ||
      receipt.tag !== tag ||
      receipt.target !== platform.target ||
      receipt.asset !== platform.asset
    )
      throw new Error('Build receipt does not match the expected release platform');
    seen.add(receipt.platform);
    const actual = artifacts.get(receipt.asset);
    if (
      !actual ||
      !/^[a-f0-9]{64}$/u.test(receipt.sha256) ||
      !Number.isSafeInteger(receipt.sizeBytes) ||
      receipt.sizeBytes <= 0 ||
      receipt.sha256 !== actual.sha256 ||
      receipt.sizeBytes !== actual.sizeBytes
    )
      throw new Error(`Installer bytes do not match the build receipt: ${receipt.asset}`);
  }
  return {
    schemaVersion: 1,
    tag,
    sourceSha,
    assets: expected.map((platform) => {
      const receipt = receipts.find((item) => item.platform === platform.platform);
      return {
        platform: receipt.platform,
        target: receipt.target,
        asset: receipt.asset,
        sha256: receipt.sha256,
        sizeBytes: receipt.sizeBytes,
      };
    }),
  };
}
