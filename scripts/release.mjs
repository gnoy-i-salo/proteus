import {
  readFileSync,
  readdirSync,
  writeFileSync,
  appendFileSync,
  mkdirSync,
  copyFileSync,
} from 'node:fs';
import { execFileSync } from 'node:child_process';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import {
  validateRelease,
  requiredAssets,
  releasePlatforms,
  validateReleaseReadiness,
  validateSourceSha,
  hashArtifact,
  verifyBuildReceipts,
} from './release-provenance.mjs';
export { validateRelease, requiredAssets } from './release-provenance.mjs';

function checkoutSha() {
  const actual = execFileSync('git', ['rev-parse', '--verify', 'HEAD^{commit}'], {
    encoding: 'utf8',
  }).trim();
  return validateSourceSha(actual, actual);
}
function collect(tag, sourceSha) {
  const { TARGET: target, PLATFORM: platform, EXTENSION: extension } = process.env;
  const configuration = releasePlatforms(tag).find((item) => item.platform === platform);
  if (!configuration || configuration.target !== target || configuration.extension !== extension)
    throw new Error('Unsupported release platform/target/extension');
  const root = `apps/desktop/src-tauri/target/${target}/release/bundle`;
  const installers = readdirSync(root, { recursive: true }).filter((name) =>
    name.endsWith(`.${extension}`),
  );
  if (installers.length !== 1)
    throw new Error(`Expected one ${platform} installer, found ${installers.length}`);
  const input = `${root}/${installers[0]}`;
  const expected = hashArtifact(input);
  mkdirSync('release-assets', { recursive: true });
  const destination = `release-assets/${configuration.asset}`;
  copyFileSync(input, destination);
  const actual = hashArtifact(destination);
  if (actual.sha256 !== expected.sha256 || actual.sizeBytes !== expected.sizeBytes)
    throw new Error('Installer changed during collection');
  const receipt = {
    schemaVersion: 1,
    tag,
    sourceSha,
    platform,
    target,
    asset: configuration.asset,
    ...actual,
  };
  writeFileSync(`${destination}.source.json`, `${JSON.stringify(receipt, null, 2)}\n`);
}
function checksums(tag, sourceSha) {
  const found = readdirSync('release-assets');
  const files = requiredAssets(tag, found);
  const allowed = new Set([
    ...files,
    ...files.map((name) => `${name}.source.json`),
    'SHA256SUMS.txt',
    'BUILD-MANIFEST.json',
  ]);
  if (found.some((name) => !allowed.has(name))) throw new Error('Unexpected release artifact');
  const receipts = files.map((name) =>
    JSON.parse(readFileSync(`release-assets/${name}.source.json`, 'utf8')),
  );
  const artifacts = new Map(files.map((name) => [name, hashArtifact(`release-assets/${name}`)]));
  const manifest = verifyBuildReceipts(tag, sourceSha, receipts, artifacts);
  writeFileSync('release-assets/BUILD-MANIFEST.json', `${JSON.stringify(manifest, null, 2)}\n`);
  artifacts.set('BUILD-MANIFEST.json', hashArtifact('release-assets/BUILD-MANIFEST.json'));
  const lines = [...artifacts].map(([name, digest]) => `${digest.sha256}  ${name}`);
  writeFileSync('release-assets/SHA256SUMS.txt', `${lines.join('\n')}\n`);
}
if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const tag = process.env.RELEASE_TAG ?? '';
  const json = (file) => JSON.parse(readFileSync(file, 'utf8'));
  const cargoToml = readFileSync('apps/desktop/src-tauri/Cargo.toml', 'utf8');
  const cargo = cargoToml.match(/^version = "([^"]+)"/mu)?.[1];
  const name = cargoToml.match(/^name = "([^"]+)"/mu)?.[1];
  if (!name) throw new Error('Missing native application package name');
  const locked = readFileSync('apps/desktop/src-tauri/Cargo.lock', 'utf8')
    .split(/^\[\[package\]\]$/mu)
    .filter(
      (block) => block.match(/^name = "([^"]+)"/mu)?.[1] === name && !/^source = /mu.test(block),
    );
  const lockedVersion =
    locked.length === 1 ? locked[0].match(/^version = "([^"]+)"/mu)?.[1] : undefined;
  validateRelease(tag, [
    json('package.json').version,
    json('apps/desktop/package.json').version,
    json('apps/desktop/src-tauri/tauri.conf.json').version,
    cargo,
    lockedVersion,
  ]);
  const issues = readFileSync('.beads/issues.jsonl', 'utf8')
    .split('\n')
    .filter((line) => line.trim())
    .map((line) => JSON.parse(line));
  validateReleaseReadiness(issues);
  const mode = process.argv[2];
  if (mode === 'source') {
    const sourceSha = checkoutSha();
    if (process.env.GITHUB_OUTPUT)
      appendFileSync(process.env.GITHUB_OUTPUT, `sourceSha=${sourceSha}\n`);
    console.log(`Release source ${sourceSha} verified`);
  } else if (mode === 'collect' || mode === 'checksums') {
    const sourceSha = validateSourceSha(checkoutSha(), process.env.SOURCE_SHA);
    if (mode === 'collect') collect(tag, sourceSha);
    else checksums(tag, sourceSha);
  } else if (mode !== undefined) throw new Error('Unsupported release verification command');
  console.log(`Release ${tag} verified`);
}
