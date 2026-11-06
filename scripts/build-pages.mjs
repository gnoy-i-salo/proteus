import { execFileSync, spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { join } from 'node:path';
import { assemblePages } from './pages-artifact.mjs';

const root = fileURLToPath(new URL('../', import.meta.url));
const diagramsPath = join(root, '.cache/pages-architecture');
function run(args) {
  const result = spawnSync('pnpm', args, {
    cwd: root,
    stdio: 'inherit',
    shell: process.platform === 'win32',
  });
  if (result.error) throw result.error;
  if (result.status !== 0) process.exit(result.status ?? 1);
}

run(['--filter', '@proteus-int/auth', 'build']);
run([
  'exec',
  'likec4',
  'build',
  'architecture',
  '--output',
  diagramsPath,
  '--base',
  '/proteus/architecture/',
  '--use-hash-history',
  '--title',
  'PROTEUS architecture',
]);
const revision = execFileSync('git', ['rev-parse', 'HEAD'], { cwd: root, encoding: 'utf8' }).trim();
assemblePages({
  authPath: join(root, 'apps/auth/dist'),
  diagramsPath,
  outputPath: join(root, '.cache/pages'),
  revision,
});
console.log('Pages artifact ready: .cache/pages (auth + architecture)');
