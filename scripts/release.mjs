import { readFileSync, readdirSync, writeFileSync, mkdirSync, copyFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
export function validateRelease(tag, versions) {
  if (!/^v\d+\.\d+\.\d+$/u.test(tag)) throw new Error('Expected a stable version tag: vX.Y.Z');
  const version = tag.slice(1);
  if (!versions.length || versions.some((value) => value !== version))
    throw new Error('Release tag must match all application versions');
  return version;
}
export function requiredAssets(tag, files) {
  const expected = ['macos-arm64.dmg', 'macos-x64.dmg', 'windows-x64.exe', 'linux-x64.deb'].map(
    (suffix) => `proteus-${tag}-${suffix}`,
  );
  for (const name of expected)
    if (!files.includes(name)) throw new Error(`Missing release asset: ${name}`);
  return expected;
}
if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const tag = process.env.RELEASE_TAG ?? '';
  const json = (file) => JSON.parse(readFileSync(file, 'utf8'));
  const cargo = readFileSync('apps/desktop/src-tauri/Cargo.toml', 'utf8').match(
    /^version = "([^"]+)"/mu,
  )?.[1];
  validateRelease(tag, [
    json('package.json').version,
    json('apps/desktop/package.json').version,
    json('apps/desktop/src-tauri/tauri.conf.json').version,
    cargo,
  ]);
  if (process.argv[2] === 'collect') {
    const { TARGET: target, PLATFORM: platform, EXTENSION: extension } = process.env;
    const root = `apps/desktop/src-tauri/target/${target}/release/bundle`;
    const installers = readdirSync(root, { recursive: true }).filter((name) =>
      name.endsWith(`.${extension}`),
    );
    if (installers.length !== 1)
      throw new Error(`Expected one ${platform} installer, found ${installers.length}`);
    mkdirSync('release-assets', { recursive: true });
    copyFileSync(
      `${root}/${installers[0]}`,
      `release-assets/proteus-${tag}-${platform}.${extension}`,
    );
  }
  if (process.argv[2] === 'checksums') {
    const files = requiredAssets(tag, readdirSync('release-assets'));
    const lines = files.map(
      (name) =>
        `${createHash('sha256')
          .update(readFileSync(`release-assets/${name}`))
          .digest('hex')}  ${name}`,
    );
    writeFileSync('release-assets/SHA256SUMS.txt', `${lines.join('\n')}\n`);
  }
  console.log(`Release ${tag} verified`);
}
