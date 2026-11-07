import { test } from 'node:test';
import assert from 'node:assert/strict';
import { copyFileSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { delimiter, join } from 'node:path';
import { spawnSync } from 'node:child_process';

function selectiveCommands(packageName) {
  const directory = mkdtempSync(join(tmpdir(), 'proteus-check-fixture-'));
  try {
    mkdirSync(join(directory, 'scripts'));
    for (const filename of ['check.mjs', 'workspace.mjs'])
      copyFileSync(new URL(filename, import.meta.url), join(directory, 'scripts', filename));
    for (const [path, name, dependencies] of [
      ['packages/contracts', '@proteus-int/contracts', {}],
      ['apps/desktop', '@proteus-int/desktop', { '@proteus-int/contracts': 'workspace:*' }],
      ['apps/auth', '@proteus-int/auth', { '@proteus-int/contracts': 'workspace:*' }],
    ]) {
      mkdirSync(join(directory, path, 'src'), { recursive: true });
      writeFileSync(join(directory, path, 'package.json'), JSON.stringify({ name, dependencies }));
    }
    const bin = join(directory, 'bin');
    mkdirSync(bin);
    const runner = join(bin, 'pnpm-fixture.cjs');
    const log = join(directory, 'commands.jsonl');
    writeFileSync(
      runner,
      "require('node:fs').appendFileSync(process.env.PROTEUS_CHECK_LOG, JSON.stringify(process.argv.slice(2)) + '\\n');\n",
    );
    const quote = (value) => `'${value.replaceAll("'", "'\\''")}'`;
    writeFileSync(
      join(bin, 'pnpm'),
      `#!/bin/sh\nexec ${quote(process.execPath)} ${quote(runner)} "$@"\n`,
      { mode: 0o755 },
    );
    writeFileSync(join(bin, 'pnpm.cmd'), `@echo off\r\n"${process.execPath}" "${runner}" %*\r\n`);
    const result = spawnSync(process.execPath, ['scripts/check.mjs', '--package', packageName], {
      cwd: directory,
      env: {
        ...process.env,
        PATH: `${bin}${delimiter}${process.env.PATH ?? ''}`,
        PROTEUS_CHECK_LOG: log,
      },
      encoding: 'utf8',
    });
    assert.equal(result.status, 0, result.stderr || result.stdout);
    return readFileSync(log, 'utf8')
      .trim()
      .split('\n')
      .map((line) => JSON.parse(line));
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
}

test('auth-only checks cannot omit frontend style validation or its production build', () => {
  const commands = selectiveCommands('auth');
  assert(
    commands.some((command) => command[0] === 'lint:styles'),
    'auth requires style checks',
  );
  assert(
    commands.some((command) => command.join(' ') === '--filter @proteus-int/auth build'),
    'auth requires its build',
  );
  assert(!commands.some((command) => command.join(' ') === '--filter @proteus-int/desktop build'));
});

test('a shared contract change builds every affected frontend', () => {
  const commands = selectiveCommands('contracts');
  for (const name of ['desktop', 'auth'])
    assert(
      commands.some((command) => command.join(' ') === `--filter @proteus-int/${name} build`),
      `${name} must be built`,
    );
});

test('selective frontend checks include e2e/config lint and the actual tooling type project', () => {
  for (const name of ['desktop', 'auth']) {
    const commands = selectiveCommands(name);
    assert(
      commands.some((command) => command.join(' ') === `exec eslint --max-warnings=0 apps/${name}`),
      `${name} must lint the complete package including its test/config files`,
    );
    assert(
      commands.some((command) => command.join(' ') === 'exec tsc -p tooling/tsconfig.json'),
      `${name} must check browser fixture and runner types`,
    );
  }
});
