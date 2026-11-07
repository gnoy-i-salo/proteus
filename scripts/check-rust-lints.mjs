import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';

const manifest = readFileSync(
  new URL('../apps/desktop/src-tauri/Cargo.toml', import.meta.url),
  'utf8',
);
const profile = manifest.match(/^\[lints\.clippy\]\r?\n([\s\S]*?)(?=^\[|(?![\s\S]))/m)?.[1] ?? '';
const root = mkdtempSync(join(tmpdir(), 'proteus-rust-lints-'));
try {
  mkdirSync(join(root, 'src'));
  writeFileSync(
    join(root, 'Cargo.toml'),
    `[package]\nname = "proteus-lint-fixture"\nversion = "0.0.0"\nedition = "2021"\n\n[lints.clippy]\n${profile}`,
  );
  const check = (source) => {
    writeFileSync(join(root, 'src/lib.rs'), source);
    const result = spawnSync(
      'cargo',
      [
        'clippy',
        '--offline',
        '--manifest-path',
        join(root, 'Cargo.toml'),
        '--all-targets',
        '--message-format=json',
        '--',
        '-D',
        'warnings',
      ],
      {
        env: { ...process.env, CARGO_TARGET_DIR: join(root, 'target') },
        encoding: 'utf8',
      },
    );
    if (result.error) throw result.error;
    return result;
  };
  const valid = check('pub fn identity(value: u8) -> u8 { value }\n');
  assert.equal(valid.status, 0, valid.stderr);
  for (const [lint, source] of [
    ['dbg_macro', 'pub fn identity(value: u8) -> u8 { dbg!(value) }\n'],
    ['todo', 'pub fn identity(_: u8) -> u8 { todo!() }\n'],
    ['unimplemented', 'pub fn identity(_: u8) -> u8 { unimplemented!() }\n'],
  ]) {
    const rejected = check(source);
    assert.notEqual(rejected.status, 0, `Cargo lint profile must reject ${lint}`);
    const diagnostics = rejected.stdout
      .trim()
      .split('\n')
      .filter(Boolean)
      .map((line) => JSON.parse(line));
    assert.ok(
      diagnostics.some(
        (item) =>
          item.reason === 'compiler-message' && item.message?.code?.code === `clippy::${lint}`,
      ),
      rejected.stderr,
    );
    console.log(`Rust lint fixture rejected ${lint}`);
  }
} finally {
  rmSync(root, { recursive: true, force: true });
}
