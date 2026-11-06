import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, copyFileSync, writeFileSync, rmSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { spawnSync } from 'node:child_process';

test('rejects an altered TDLib archive even when a verified source marker exists', () => {
  const root = mkdtempSync(join(tmpdir(), 'proteus-tdlib-'));
  try {
    mkdirSync(join(root, 'scripts'));
    const cache = join(root, '.cache');
    mkdirSync(join(cache, 'td-4c6377aa7386c39217878a9a56d581922ad37ca4'), { recursive: true });
    writeFileSync(join(cache, 'tdlib-source.tar.gz'), 'untrusted archive');
    writeFileSync(
      join(cache, 'td-4c6377aa7386c39217878a9a56d581922ad37ca4/.proteus-verified'),
      'forged',
    );
    copyFileSync(
      new URL('./prepare-tdlib.mjs', import.meta.url),
      join(root, 'scripts/prepare-tdlib.mjs'),
    );
    const result = spawnSync(process.execPath, [join(root, 'scripts/prepare-tdlib.mjs')], {
      encoding: 'utf8',
    });
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /checksum mismatch/u);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
