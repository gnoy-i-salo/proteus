import { readFileSync } from 'node:fs';
import { walk, workspaces } from './workspace.mjs';
import { fieldNamingErrors } from './code-rules.mjs';
const exceptions = new Set([
  'README.md',
  'AGENTS.md',
  'SKILL.md',
  'PULL_REQUEST_TEMPLATE.md',
  'Cargo.toml',
  'Cargo.lock',
  'LICENSE',
]);
const platformIcons = new Set([
  '128x128@2x.png',
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
  if (file.startsWith('.husky/_/')) continue;
  const parts = file.split('/');
  for (const [index, part] of parts.entries()) {
    const filename = index === parts.length - 1;
    const requiredFilename = filename && exceptions.has(part);
    const platformIcon =
      filename && file.startsWith('apps/desktop/src-tauri/icons/') && platformIcons.has(part);
    if (!requiredFilename && !platformIcon && !/^\.?[a-z0-9]+(?:[-.][a-z0-9]+)*$/.test(part))
      errors.push(`${file}: non-kebab-case segment ${part}`);
  }
  if (/^(packages|apps|tests)\//.test(file) && /\.tsx?$/.test(file))
    errors.push(...fieldNamingErrors(readFileSync(file, 'utf8'), file));
}
for (const pkg of workspaces())
  if (!/^@proteus-int\/[a-z]+(?:-[a-z]+)*$/.test(pkg.name))
    errors.push(`Invalid package name: ${pkg.name}`);
if (errors.length) {
  console.error([...new Set(errors)].join('\n'));
  process.exitCode = 1;
} else console.log('Naming OK: kebab-case paths, camelCase owned fields and @proteus-int scope.');
