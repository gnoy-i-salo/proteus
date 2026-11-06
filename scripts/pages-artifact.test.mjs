import assert from 'node:assert/strict';
import {
  mkdtempSync,
  mkdirSync,
  writeFileSync,
  readFileSync,
  existsSync,
  symlinkSync,
  rmSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';
import { assemblePages } from './pages-artifact.mjs';

function fixture(t) {
  const root = mkdtempSync(join(tmpdir(), 'proteus-pages-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const authPath = join(root, 'auth-dist');
  const diagramsPath = join(root, 'diagrams-dist');
  const outputPath = join(root, 'site');
  mkdirSync(authPath);
  mkdirSync(diagramsPath);
  writeFileSync(join(authPath, 'index.html'), '<title>VK callback</title>');
  writeFileSync(join(authPath, 'auth.js'), 'export const ready = true;');
  writeFileSync(join(diagramsPath, 'index.html'), '<title>Architecture</title>');
  writeFileSync(join(root, '.env'), 'test-only-do-not-publish');
  return { root, authPath, diagramsPath, outputPath, revision: 'a'.repeat(40) };
}

test('Pages publishes both built sites with a physical auth entry and revision, excluding workspace files', (t) => {
  const input = fixture(t);
  assemblePages(input);
  assert.equal(
    readFileSync(join(input.outputPath, 'auth/index.html'), 'utf8'),
    '<title>VK callback</title>',
  );
  assert.match(readFileSync(join(input.outputPath, 'auth/auth.js'), 'utf8'), /ready/);
  assert.equal(
    readFileSync(join(input.outputPath, 'architecture/index.html'), 'utf8'),
    '<title>Architecture</title>',
  );
  assert.equal(existsSync(join(input.outputPath, '.nojekyll')), true);
  assert.equal(existsSync(join(input.outputPath, '.env')), false);
  assert.match(readFileSync(join(input.outputPath, 'index.html'), 'utf8'), /href="\.\/auth\/"/);
  assert.match(
    readFileSync(join(input.outputPath, 'index.html'), 'utf8'),
    /href="\.\/architecture\/"/,
  );
  assert.deepEqual(JSON.parse(readFileSync(join(input.outputPath, 'build-info.json'), 'utf8')), {
    revision: input.revision,
  });
});

test('missing auth build fails before replacing the previous artifact', (t) => {
  const input = fixture(t);
  rmSync(join(input.authPath, 'index.html'));
  mkdirSync(input.outputPath);
  writeFileSync(join(input.outputPath, 'previous.txt'), 'keep');
  assert.throws(() => assemblePages(input), /Missing auth index/);
  assert.equal(readFileSync(join(input.outputPath, 'previous.txt'), 'utf8'), 'keep');
});

test('Pages rejects symlinks instead of publishing files outside a build', (t) => {
  const input = fixture(t);
  symlinkSync(join(input.root, '.env'), join(input.authPath, 'linked-file'));
  assert.throws(() => assemblePages(input), /Symbolic links/);
  assert.equal(existsSync(input.outputPath), false);
});

test('a rebuilt artifact replaces stale files and refuses invalid revision metadata', (t) => {
  const input = fixture(t);
  assert.throws(() => assemblePages({ ...input, revision: '<unsafe>' }), /Invalid revision/);
  mkdirSync(input.outputPath);
  writeFileSync(join(input.outputPath, 'stale.js'), 'old');
  assemblePages(input);
  assert.equal(existsSync(join(input.outputPath, 'stale.js')), false);
});
