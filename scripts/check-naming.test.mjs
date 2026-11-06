import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';
import test from 'node:test';

test('required SKILL.md is allowed while sibling and folder names remain checked', (t) => {
  const root = mkdtempSync(join(tmpdir(), 'proteus-naming-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  for (const path of ['apps', 'packages', 'skills/telegram-research'])
    mkdirSync(join(root, path), { recursive: true });
  writeFileSync(
    join(root, 'skills/telegram-research/SKILL.md'),
    '---\nname: telegram-research\n---',
  );
  const check = () =>
    spawnSync(process.execPath, [fileURLToPath(new URL('./check-naming.mjs', import.meta.url))], {
      cwd: root,
      encoding: 'utf8',
    });
  const allowed = check();
  assert.equal(allowed.status, 0, allowed.stderr);
  writeFileSync(join(root, 'skills/telegram-research/BadFile.md'), 'invalid sibling');
  mkdirSync(join(root, 'skills/BadFolder'));
  writeFileSync(join(root, 'skills/BadFolder/SKILL.md'), 'invalid folder');
  const rejected = check();
  assert.equal(rejected.status, 1);
  assert.match(rejected.stderr, /BadFile\.md/);
  assert.match(rejected.stderr, /BadFolder/);
});
