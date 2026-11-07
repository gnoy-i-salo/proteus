import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';
import test from 'node:test';

function project(t, path, name) {
  const root = mkdtempSync(join(tmpdir(), 'proteus-code-boundary-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  for (const folder of ['apps', 'packages', '.cache', `${path}/src`])
    mkdirSync(join(root, folder), { recursive: true });
  writeFileSync(join(root, path, 'package.json'), JSON.stringify({ name, dependencies: {} }));
  writeFileSync(
    join(root, '.cache/architecture.json'),
    JSON.stringify({
      elements: { one: { id: 'one', kind: 'module', metadata: { package: name, path } } },
      relations: {},
    }),
  );
  return {
    root,
    check: () =>
      spawnSync(
        process.execPath,
        [fileURLToPath(new URL('./check-architecture.mjs', import.meta.url))],
        { cwd: root, encoding: 'utf8' },
      ),
  };
}

test('real architecture command rejects implementation in public index', (t) => {
  const { root, check } = project(t, 'packages/contracts', '@proteus-int/contracts');
  const entry = join(root, 'packages/contracts/src/index.ts');
  writeFileSync(entry, "export { value } from './value';\n");
  assert.equal(check().status, 0);
  writeFileSync(entry, 'export function value() { return 1; }\n');
  const bad = check();
  assert.equal(bad.status, 1);
  assert.match(bad.stderr, /explicit reexports only/);
});

test('real architecture command enforces UI-to-model boundary while retaining type imports', (t) => {
  const { root, check } = project(t, 'apps/desktop', '@proteus-int/desktop');
  const folder = join(root, 'apps/desktop/src/features/example/ui');
  mkdirSync(folder, { recursive: true });
  const file = join(folder, 'example.tsx');
  writeFileSync(file, "import type { CliStatus } from '../../../shared/lib/cli';\n");
  assert.equal(check().status, 0);
  writeFileSync(file, "import { createCliProvider } from '../../../shared/lib/cli';\n");
  const bad = check();
  assert.equal(bad.status, 1);
  assert.match(bad.stderr, /UI must call its model/);
});
