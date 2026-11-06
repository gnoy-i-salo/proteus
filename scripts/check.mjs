import { spawnSync, execFileSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import { affectedPackages, workspaces } from './workspace.mjs';
const args = process.argv.slice(2);
const mode = args[0] ?? '--all';
const packages = workspaces();
function git(...args) {
  return execFileSync('git', args, { encoding: 'utf8' }).split('\0').filter(Boolean);
}
function run(...args) {
  console.log(`\n→ pnpm ${args.join(' ')}`);
  const result = spawnSync('pnpm', args, { stdio: 'inherit', shell: process.platform === 'win32' });
  if (result.error) throw result.error;
  if (result.status !== 0) process.exit(result.status ?? 1);
}
let files = [];
let selected = packages;
let all = mode === '--all';
if (mode === '--package') {
  const name = args[1]?.replace('@proteus-int/', '');
  const pkg = packages.find((item) => item.name === `@proteus-int/${name}`);
  if (!pkg) throw new Error('Usage: pnpm check:package <package-name>');
  files = [`${pkg.path}/`];
  selected = affectedPackages(packages, files);
} else if (mode === '--staged' || mode === '--changed') {
  files = git('diff', '--cached', '--name-only', '-z', '--diff-filter=ACMRD');
  if (mode === '--changed') {
    files.push(
      ...git('diff', '--name-only', '-z'),
      ...git('ls-files', '--others', '--exclude-standard', '-z'),
    );
    const base = args[1] ?? 'origin/trunk';
    if (spawnSync('git', ['rev-parse', '--verify', base], { stdio: 'ignore' }).status === 0)
      files.push(...git('diff', '--name-only', '-z', `${base}...HEAD`));
    else if (args[1]) throw new Error(`Unknown comparison base: ${base}`);
  } else {
    const unstaged = new Set(git('diff', '--name-only', '-z'));
    if (files.some((file) => unstaged.has(file)))
      throw new Error(
        'Partially staged files detected. Stage the complete change before validating the commit.',
      );
  }
  files = [...new Set(files)];
  if (files.length === 0) {
    console.log('No changed files.');
    process.exit(0);
  }
  all = files.some(
    (file) =>
      !packages.some((pkg) => file.startsWith(`${pkg.path}/`)) &&
      !file.startsWith('docs/') &&
      !file.startsWith('.beads/') &&
      file !== 'README.md',
  );
  selected = all ? packages : affectedPackages(packages, files);
} else if (!all) throw new Error(`Unknown check mode: ${mode}`);
console.log(
  all
    ? 'Full repository checks'
    : `Affected: ${selected.map((pkg) => pkg.name).join(', ') || 'documentation only'}`,
);
const targets = all
  ? ['.']
  : mode === '--package'
    ? selected.map((pkg) => pkg.path)
    : files.filter(existsSync);
if (targets.length) run('exec', 'prettier', '--check', '--ignore-unknown', ...targets);
run('check:naming');
if (all || selected.length) {
  run('check:architecture');
  run('test:harness');
  const lintTargets = all ? ['.'] : selected.map((pkg) => `${pkg.path}/src`);
  run('exec', 'eslint', '--max-warnings=0', ...lintTargets);
  const frontends = selected.filter((pkg) => pkg.path.startsWith('apps/'));
  if (all || frontends.length) run('lint:styles');
  if (all) {
    run('typecheck');
    run('test');
    run('build');
  } else {
    for (const pkg of selected) run('--filter', pkg.name, 'typecheck');
    run('exec', 'vitest', 'run', '--passWithNoTests', ...selected.map((pkg) => pkg.path));
    for (const pkg of frontends) run('--filter', pkg.name, 'build');
  }
}
if (
  mode !== '--all' &&
  files.some(
    (file) => file.startsWith('apps/desktop/src-tauri/') && /\.(rs|toml|json|lock)$/.test(file),
  )
)
  run('check:rust');
