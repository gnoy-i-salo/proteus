import { test } from 'node:test';
import assert from 'node:assert/strict';
import { copyFileSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';

function discover(prepare) {
  const directory = mkdtempSync(join(tmpdir(), 'proteus-workspace-fixture-'));
  try {
    copyFileSync(new URL('workspace.mjs', import.meta.url), join(directory, 'workspace.mjs'));
    mkdirSync(join(directory, 'apps', 'desktop'), { recursive: true });
    mkdirSync(join(directory, 'packages'));
    writeFileSync(
      join(directory, 'apps', 'desktop', 'package.json'),
      '{"name":"@proteus-int/desktop"}',
    );
    prepare(directory);
    return spawnSync(
      process.execPath,
      [
        '--input-type=module',
        '-e',
        'import { workspaces } from "./workspace.mjs"; console.log(JSON.stringify(workspaces()));',
      ],
      { cwd: directory, encoding: 'utf8' },
    );
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
}
test('workspace discovery preserves ordinary sibling files without treating them as packages', () => {
  const result = discover((directory) => {
    writeFileSync(join(directory, 'apps', '.DS_Store'), 'finder metadata');
    writeFileSync(join(directory, 'packages', 'notes.txt'), 'ordinary file');
  });
  assert.equal(result.status, 0, result.stderr);
  assert.deepEqual(JSON.parse(result.stdout), [
    { path: 'apps/desktop', name: '@proteus-int/desktop' },
  ]);
});
test('workspace discovery still rejects a directory without a valid package manifest', () => {
  for (const manifest of [null, '{broken']) {
    const result = discover((directory) => {
      const path = join(directory, 'packages', 'broken');
      mkdirSync(path);
      if (manifest !== null) writeFileSync(join(path, 'package.json'), manifest);
    });
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, manifest === null ? /ENOENT/ : /SyntaxError/);
  }
});

test('source walks ignore only generated Finder files and retain invalid source names', async () => {
  const { walk } = await import('./workspace.mjs');
  const directory = mkdtempSync(join(tmpdir(), 'proteus-walk-fixture-'));
  try {
    writeFileSync(join(directory, '.DS_Store'), 'metadata');
    writeFileSync(join(directory, 'BadName.ts'), 'export const value = 1;');
    mkdirSync(join(directory, 'nested', '.DS_Store'), { recursive: true });
    writeFileSync(join(directory, 'nested', '.DS_Store', 'bad.ts'), 'export const value = 2;');
    assert.deepEqual(
      walk(directory).sort(),
      [join(directory, 'BadName.ts'), join(directory, 'nested', '.DS_Store', 'bad.ts')]
        .map((path) => path.replaceAll('\\', '/'))
        .sort(),
    );
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});
