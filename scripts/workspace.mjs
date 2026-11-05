import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
export const ignored = new Set([
  'node_modules',
  '.git',
  '.cache',
  'target',
  'dist',
  'gen',
  '.beads',
  'coverage',
  'test-results',
  'playwright-report',
]);
export function walk(directory) {
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    if (ignored.has(entry.name)) return [];
    const path = join(directory, entry.name).replaceAll('\\', '/');
    return entry.isDirectory() ? walk(path) : [path];
  });
}
export function readJson(path) {
  return JSON.parse(readFileSync(path, 'utf8'));
}
export function workspaces() {
  return ['apps', 'packages'].flatMap((parent) =>
    readdirSync(parent).map((name) => {
      const path = `${parent}/${name}`;
      return { path, ...readJson(`${path}/package.json`) };
    }),
  );
}
export function affectedPackages(packages, changed) {
  const selected = new Set(
    packages
      .filter((pkg) => changed.some((file) => file.startsWith(`${pkg.path}/`)))
      .map((pkg) => pkg.name),
  );
  let grew = true;
  while (grew) {
    grew = false;
    for (const pkg of packages) {
      if (
        !selected.has(pkg.name) &&
        Object.keys({ ...pkg.dependencies, ...pkg.devDependencies }).some((dep) =>
          selected.has(dep),
        )
      ) {
        selected.add(pkg.name);
        grew = true;
      }
    }
  }
  return packages.filter((pkg) => selected.has(pkg.name));
}
