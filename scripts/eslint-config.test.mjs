import assert from 'node:assert/strict';
import { cpSync, mkdirSync, mkdtempSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import test from 'node:test';
import { ESLint } from 'eslint';

const repoRoot = fileURLToPath(new URL('..', import.meta.url));

function fixtureProject(t) {
  const root = mkdtempSync(join(tmpdir(), 'proteus-eslint-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  cpSync(join(repoRoot, 'eslint.config.mjs'), join(root, 'eslint.config.mjs'));
  symlinkSync(join(repoRoot, 'node_modules'), join(root, 'node_modules'), 'dir');
  writeFileSync(join(root, 'package.json'), JSON.stringify({ type: 'module' }));
  writeFileSync(
    join(root, 'tsconfig.json'),
    JSON.stringify({
      compilerOptions: { strict: true, target: 'ES2022', module: 'ESNext' },
      include: ['src/**/*.ts'],
    }),
  );
  const eslint = new ESLint({
    cwd: root,
    overrideConfigFile: join(root, 'eslint.config.mjs'),
    overrideConfig: { languageOptions: { parserOptions: { tsconfigRootDir: root } } },
  });
  return async (name, source) => {
    const path = join(root, 'src', name);
    mkdirSync(dirname(path), { recursive: true });
    writeFileSync(path, source);
    const [result] = await eslint.lintFiles([path]);
    assert.equal(result.fatalErrorCount, 0, JSON.stringify(result.messages));
    return result.messages;
  };
}

test('type-aware rules reject unhandled and void-discarded promises', async (t) => {
  const lint = fixtureProject(t);
  for (const source of ['Promise.resolve();', 'void Promise.resolve();']) {
    const messages = await lint(`promise-${source.startsWith('void')}.ts`, source);
    assert.ok(
      messages.some(({ ruleId }) => ruleId === '@typescript-eslint/no-floating-promises'),
      JSON.stringify(messages),
    );
  }
});

test('type-aware rules reject unsafe JSON and async void callbacks', async (t) => {
  const lint = fixtureProject(t);
  const unsafe = await lint('unsafe.ts', 'export const value = JSON.parse("{}").result;');
  assert.ok(
    unsafe.some(({ ruleId }) => ruleId === '@typescript-eslint/no-unsafe-member-access'),
    JSON.stringify(unsafe),
  );
  const misused = await lint(
    'callback.ts',
    'export const handler: () => void = () => Promise.resolve();',
  );
  assert.ok(
    misused.some(({ ruleId }) => ruleId === '@typescript-eslint/no-misused-promises'),
    JSON.stringify(misused),
  );
});

test('handled promises and unknown data are allowed; JS does not require a TS project', async (t) => {
  const lint = fixtureProject(t);
  const valid = await lint(
    'handled.ts',
    'export const value: unknown = JSON.parse("{}");\nvoid Promise.resolve().catch((error: unknown) => { console.error(error); });',
  );
  assert.deepEqual(valid, []);
  const javascript = await lint('tool.mjs', 'export const version = process.version;');
  assert.deepEqual(javascript, []);
});

test('readability limits reject exceptional branch counts and five nested blocks', async (t) => {
  const lint = fixtureProject(t);
  const branches = Array.from(
    { length: 26 },
    (_, index) => `if (value === ${index}) return ${index};`,
  ).join('\n');
  const complex = await lint(
    'complex.ts',
    `export function select(value: number) {\n${branches}\nreturn -1;\n}`,
  );
  assert.ok(
    complex.some(({ ruleId }) => ruleId === 'complexity'),
    JSON.stringify(complex),
  );
  const deep = await lint(
    'deep.ts',
    'export function nested(value: number) { if (value > 0) { if (value > 1) { if (value > 2) { if (value > 3) { if (value > 4) { return value; } } } } } return 0; }',
  );
  assert.ok(
    deep.some(({ ruleId }) => ruleId === 'max-depth'),
    JSON.stringify(deep),
  );
});

test('all intended TypeScript surfaces use project service and keep Solid rules', async () => {
  const eslint = new ESLint({ cwd: repoRoot });
  for (const path of [
    'packages/contracts/src/corpus.ts',
    'apps/desktop/src/app/app.tsx',
    'apps/auth/src/main.ts',
    'apps/desktop/e2e/experiment.spec.ts',
    'tests/e2e/auth.spec.ts',
    'vitest.config.ts',
  ]) {
    const config = await eslint.calculateConfigForFile(join(repoRoot, path));
    assert.equal(config.languageOptions.parserOptions.projectService, true, path);
    assert.equal(config.rules['@typescript-eslint/no-explicit-any'][0], 2, path);
    assert.equal(config.rules['@typescript-eslint/consistent-type-imports'][0], 2, path);
    if (path.endsWith('.tsx')) assert.ok(config.plugins.solid, path);
  }
});
