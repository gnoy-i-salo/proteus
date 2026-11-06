import { createHash } from 'node:crypto';
import { existsSync, mkdirSync, readFileSync, writeFileSync, rmSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { join } from 'node:path';

const revision = '4c6377aa7386c39217878a9a56d581922ad37ca4';
const sha256 = '472015605fffa826bfba6ccf7d888905b436ecce94c8a6826203792edfbe5731';
const cache = fileURLToPath(new URL('../.cache/', import.meta.url));
const archive = join(cache, 'tdlib-source.tar.gz');
const source = join(cache, `td-${revision}`);
mkdirSync(cache, { recursive: true });
if (!existsSync(archive)) {
  const response = await fetch(`https://github.com/tdlib/td/archive/${revision}.tar.gz`);
  if (!response.ok) throw new Error(`TDLib download failed: ${response.status}`);
  writeFileSync(archive, Buffer.from(await response.arrayBuffer()));
}
if (createHash('sha256').update(readFileSync(archive)).digest('hex') !== sha256) {
  throw new Error('TDLib archive checksum mismatch. Remove .cache/tdlib-source.tar.gz and retry.');
}
if (!existsSync(join(source, '.proteus-verified'))) {
  rmSync(source, { recursive: true, force: true });
  execFileSync('tar', ['-xzf', archive, '-C', cache], { stdio: 'inherit' });
  writeFileSync(join(source, '.proteus-verified'), sha256);
}
console.log(`Verified TDLib source: ${revision}`);
