import ts from 'typescript';
import { posix } from 'node:path';
export function importsOf(source, file = 'source.ts') {
  const imports = [];
  const ast = ts.createSourceFile(file, source, ts.ScriptTarget.Latest, true);
  function visit(node) {
    if (
      (ts.isImportDeclaration(node) || ts.isExportDeclaration(node)) &&
      node.moduleSpecifier &&
      ts.isStringLiteral(node.moduleSpecifier)
    )
      imports.push(node.moduleSpecifier.text);
    if (
      ts.isImportTypeNode(node) &&
      ts.isLiteralTypeNode(node.argument) &&
      ts.isStringLiteral(node.argument.literal)
    )
      imports.push(node.argument.literal.text);
    if (
      ts.isImportEqualsDeclaration(node) &&
      ts.isExternalModuleReference(node.moduleReference) &&
      node.moduleReference.expression &&
      ts.isStringLiteral(node.moduleReference.expression)
    )
      imports.push(node.moduleReference.expression.text);
    if (
      ts.isCallExpression(node) &&
      (node.expression.kind === ts.SyntaxKind.ImportKeyword ||
        (ts.isIdentifier(node.expression) && node.expression.text === 'require'))
    ) {
      const argument = node.arguments[0];
      if (
        argument &&
        (ts.isStringLiteral(argument) || ts.isNoSubstitutionTemplateLiteral(argument))
      )
        imports.push(argument.text);
      else imports.push('<non-literal-import>');
    }
    ts.forEachChild(node, visit);
  }
  visit(ast);
  return imports;
}
export function checkImport(file, specifier, packages, edges) {
  const source = packages.find((pkg) => file.startsWith(`${pkg.path}/`));
  if (!source) return null;
  if (specifier === '<non-literal-import>')
    return 'non-literal imports cannot be architecture-checked';
  if (specifier.startsWith('@proteus-int/')) {
    const target = packages.find((pkg) => specifier === pkg.name);
    if (!target) return 'unknown package or deep import; use its public API';
    if (target.name === source.name)
      return 'self-import through package API creates cycles; use relative imports';
    if (!edges.has(`${source.name}->${target.name}`)) return 'dependency is missing from LikeC4';
    if (source.dependencies?.[target.name] !== 'workspace:*')
      return 'workspace dependency must be declared as workspace:*';
    return null;
  }
  if (!specifier.startsWith('.')) {
    if (specifier.startsWith('/') || specifier.startsWith('@/') || specifier.startsWith('~/'))
      return 'use relative paths within a package and package public APIs across packages';
    const name = specifier.startsWith('@')
      ? specifier.split('/').slice(0, 2).join('/')
      : specifier.split('/')[0];
    if (/\.(test|spec)\./.test(file) && ['vitest', '@playwright/test'].includes(name)) return null;
    if (!source.dependencies?.[name] && !source.devDependencies?.[name])
      return `undeclared external dependency: ${name}`;
    return null;
  }
  const target = posix.normalize(posix.join(posix.dirname(file), specifier));
  if (!target.startsWith(`${source.path}/`)) return 'relative import escapes its package';
  if (source.name !== '@proteus-int/desktop') return null;
  const base = `${source.path}/src/`;
  if (!file.startsWith(base)) return null;
  const [layer, slice] = file.slice(base.length).split('/');
  const [targetLayer, targetSlice, ...rest] = target.slice(base.length).split('/');
  const layers = ['shared', 'entities', 'features', 'widgets', 'pages', 'app'];
  if (layer === 'main.tsx') return targetLayer === 'app' ? null : 'entrypoint may only import app';
  if (!layers.includes(layer) || !layers.includes(targetLayer)) return 'unknown FSD layer';
  if (layers.indexOf(targetLayer) > layers.indexOf(layer)) return 'FSD upward dependency';
  if (layer === targetLayer && !['app', 'shared'].includes(layer) && slice !== targetSlice)
    return 'FSD cross-slice dependency';
  if (
    layer !== targetLayer &&
    !['app', 'shared'].includes(targetLayer) &&
    rest.length > 0 &&
    rest.join('/') !== 'index'
  )
    return 'FSD slice deep import; use index.ts';
  return null;
}
export function hasCycle(packages, edges) {
  const active = new Set();
  const done = new Set();
  function visit(name) {
    if (active.has(name)) return true;
    if (done.has(name)) return false;
    active.add(name);
    for (const pkg of packages)
      if (edges.has(`${name}->${pkg.name}`) && visit(pkg.name)) return true;
    active.delete(name);
    done.add(name);
    return false;
  }
  return packages.some((pkg) => visit(pkg.name));
}
