import { readFileSync } from 'node:fs';
import { workspaces, walk, readJson } from './workspace.mjs';
import { checkImport, hasCycle, importsOf } from './architecture-rules.mjs';
const packages = workspaces();
const model = readJson('.cache/architecture.json');
const elements = Object.values(model.elements);
const names = new Map(elements.map((element) => [element.id, element.metadata?.package]));
const edges = new Set(
  Object.values(model.relations).map(
    (relation) => `${names.get(relation.source.model)}->${names.get(relation.target.model)}`,
  ),
);
const errors = [];
for (const element of elements) {
  if (
    !packages.some(
      (pkg) => pkg.name === element.metadata?.package && pkg.path === element.metadata?.path,
    )
  )
    errors.push(`LikeC4 element ${element.id} does not match a workspace package`);
}
for (const pkg of packages) {
  if (
    !elements.some(
      (element) => element.metadata?.package === pkg.name && element.metadata?.path === pkg.path,
    )
  )
    errors.push(`${pkg.name}: missing LikeC4 element`);
  for (const [dep, version] of Object.entries({
    ...pkg.dependencies,
    ...pkg.devDependencies,
    ...pkg.peerDependencies,
    ...pkg.optionalDependencies,
  })) {
    if (
      dep.startsWith('@proteus-int/') &&
      (!packages.some((other) => other.name === dep) ||
        !edges.has(`${pkg.name}->${dep}`) ||
        version !== 'workspace:*')
    )
      errors.push(`${pkg.name}: illegal manifest dependency ${dep}`);
  }
  for (const file of walk(`${pkg.path}/src`).filter((path) => /\.(?:[cm]?[jt]sx?)$/.test(path))) {
    for (const specifier of importsOf(readFileSync(file, 'utf8'), file)) {
      const error = checkImport(file, specifier, packages, edges);
      if (error) errors.push(`${file}: ${specifier}: ${error}`);
    }
  }
}
for (const edge of edges) {
  const [from, to] = edge.split('->');
  const pkg = packages.find((item) => item.name === from);
  if (!pkg?.dependencies?.[to])
    errors.push(`LikeC4 edge ${edge} has no matching manifest dependency`);
}
if (hasCycle(packages, edges)) errors.push('LikeC4 package dependencies contain a cycle');
if (errors.length) {
  console.error(errors.join('\n'));
  process.exitCode = 1;
} else
  console.log(
    `Architecture OK: ${packages.length} packages; LikeC4 dependencies, public APIs and FSD layers verified.`,
  );
