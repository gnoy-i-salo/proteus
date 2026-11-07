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

test('real naming command rejects owned fields but preserves exact strings and skill files', (t) => {
  const root = mkdtempSync(join(tmpdir(), 'proteus-field-naming-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  for (const path of ['apps', 'packages/contracts/src', '.agents/skills/code-quality/agents'])
    mkdirSync(join(root, path), { recursive: true });
  writeFileSync(
    join(root, 'packages/contracts/package.json'),
    JSON.stringify({ name: '@proteus-int/contracts' }),
  );
  writeFileSync(join(root, '.agents/skills/code-quality/SKILL.md'), '# Skill\n');
  writeFileSync(
    join(root, '.agents/skills/code-quality/agents/openai.yaml'),
    'interface:\n  display_name: Test\n',
  );
  const file = join(root, 'packages/contracts/src/value.ts');
  const check = () =>
    spawnSync(process.execPath, [fileURLToPath(new URL('./check-naming.mjs', import.meta.url))], {
      cwd: root,
      encoding: 'utf8',
    });
  writeFileSync(file, `export const value = { exactText: '{"bad-field":"raw_words"}' };`);
  assert.equal(check().status, 0);
  writeFileSync(file, `export const value = { 'bad-field': 'exact words' };`);
  const rejected = check();
  assert.equal(rejected.status, 1);
  assert.match(rejected.stderr, /owned TypeScript field bad-field/);
});

test('platform icon exception does not exempt sibling files or uppercase skill folders', (t) => {
  const root = mkdtempSync(join(tmpdir(), 'proteus-exact-naming-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  for (const path of [
    'apps/desktop/src-tauri/icons',
    'packages',
    '.agents/skills/BadFolder/agents',
  ])
    mkdirSync(join(root, path), { recursive: true });
  writeFileSync(
    join(root, 'apps/desktop/package.json'),
    JSON.stringify({ name: '@proteus-int/desktop' }),
  );
  writeFileSync(join(root, 'apps/desktop/src-tauri/icons/128x128@2x.png'), 'platform asset');
  writeFileSync(join(root, 'apps/desktop/src-tauri/icons/BadFile.ts'), 'export const value = 1;');
  writeFileSync(join(root, '.agents/skills/BadFolder/SKILL.md'), '# Skill\n');
  const rejected = spawnSync(
    process.execPath,
    [fileURLToPath(new URL('./check-naming.mjs', import.meta.url))],
    { cwd: root, encoding: 'utf8' },
  );
  assert.equal(rejected.status, 1);
  assert.match(rejected.stderr, /BadFile/);
  assert.match(rejected.stderr, /BadFolder/);
  assert.doesNotMatch(rejected.stderr, /128x128/);
});
