import { walk, workspaces } from './workspace.mjs';
const exceptions = new Set([
  'README.md',
  'AGENTS.md',
  'SKILL.md',
  'PULL_REQUEST_TEMPLATE.md',
  'Cargo.toml',
  'Cargo.lock',
  'LICENSE',
  'Square30x30Logo.png',
  'Square44x44Logo.png',
  'Square71x71Logo.png',
  'Square89x89Logo.png',
  'Square107x107Logo.png',
  'Square142x142Logo.png',
  'Square150x150Logo.png',
  'Square284x284Logo.png',
  'Square310x310Logo.png',
  'StoreLogo.png',
]);
const errors = [];
for (const file of walk('.')) {
  // Tauri's platform-generated assets use platform-mandated names.
  if (file.startsWith('apps/desktop/src-tauri/icons/') || file.startsWith('.husky/_/')) continue;
  for (const part of file.split('/')) {
    if (!exceptions.has(part) && !/^\.?[a-z0-9]+(?:[-.][a-z0-9]+)*$/.test(part))
      errors.push(`${file}: non-kebab-case segment ${part}`);
  }
}
for (const pkg of workspaces())
  if (!/^@proteus-int\/[a-z]+(?:-[a-z]+)*$/.test(pkg.name))
    errors.push(`Invalid package name: ${pkg.name}`);
if (errors.length) {
  console.error([...new Set(errors)].join('\n'));
  process.exitCode = 1;
} else console.log('Naming OK: kebab-case paths and @proteus-int scope.');
