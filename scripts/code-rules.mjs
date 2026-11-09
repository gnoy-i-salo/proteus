import ts from 'typescript';
import { posix } from 'node:path';
import { readFileSync } from 'node:fs';

const fieldExceptions = JSON.parse(
  readFileSync(new URL('./field-naming-exceptions.json', import.meta.url), 'utf8'),
);
const camelCase = /^[a-z][a-zA-Z0-9]*$/;
const adapters = new Set([
  'cli',
  'sources',
  'source-bindings',
  'ingestion-input',
  'ingestion',
  'research-memory',
  'fabric-memory',
  'native-memory-session',
  'native-operation',
  'local-model',
  'corpus-search',
  'connections',
  'presets',
  'model-settings-storage',
]);
const orchestrationPackages = new Set([
  'pipeline',
  'person',
  'person-fabric',
  'stalker',
  'brain-analyzator',
  'brain-scoring',
]);
const windowSelectors = new Set(['isDesktop', 'windowPlatform', 'windowActions']);
const intelligenceSelectors = new Set(['componentIds', 'isProviderId']);

function parse(source, file) {
  return ts.createSourceFile(file, source, ts.ScriptTarget.Latest, true);
}
function location(ast, node) {
  return ast.getLineAndCharacterOfPosition(node.getStart(ast)).line + 1;
}
function fieldName(node) {
  if (!(
    ts.isPropertyAssignment(node) ||
    ts.isShorthandPropertyAssignment(node) ||
    ts.isPropertySignature(node) ||
    ts.isPropertyDeclaration(node) ||
    ts.isMethodDeclaration(node) ||
    ts.isMethodSignature(node) ||
    ts.isGetAccessorDeclaration(node) ||
    ts.isSetAccessorDeclaration(node)
  ))
    return null;
  const name = node.name;
  if (ts.isPrivateIdentifier(name)) return name.text.slice(1);
  if (ts.isIdentifier(name) || ts.isStringLiteral(name)) return name.text;
  if (ts.isComputedPropertyName(name)) {
    let expression = name.expression;
    while (
      ts.isParenthesizedExpression(expression) ||
      ts.isAsExpression(expression) ||
      ts.isTypeAssertionExpression(expression) ||
      ts.isSatisfiesExpression(expression)
    )
      expression = expression.expression;
    if (ts.isStringLiteral(expression) || ts.isNoSubstitutionTemplateLiteral(expression))
      return expression.text;
  }
  return null;
}
export function fieldNamingErrors(source, file) {
  const ast = parse(source, file);
  const errors = [];
  const allowed = new Set(fieldExceptions[file]?.fields ?? []);
  function visit(node) {
    const name = fieldName(node);
    if (name !== null && !camelCase.test(name) && !allowed.has(name))
      errors.push(
        `${file}:${location(ast, node)}: owned TypeScript field ${name} must use camelCase`,
      );
    ts.forEachChild(node, visit);
  }
  visit(ast);
  return errors;
}
export function publicEntryErrors(source, file) {
  if (!/^(packages|apps)\/.*\/src\/(?:.*\/)?index\.ts$/.test(file)) return [];
  const ast = parse(source, file);
  return ast.statements
    .filter(
      (node) =>
        !(
          ts.isExportDeclaration(node) &&
          node.moduleSpecifier &&
          node.exportClause &&
          ts.isNamedExports(node.exportClause)
        ),
    )
    .map(
      (node) =>
        `${file}:${location(ast, node)}: public index.ts must contain explicit reexports only`,
    );
}
function importBinding(node) {
  if (ts.isImportDeclaration(node)) {
    const clause = node.importClause;
    if (clause?.isTypeOnly) return { typeOnly: true, names: [] };
    const bindings = clause?.namedBindings;
    if (!clause?.name && bindings && ts.isNamedImports(bindings)) {
      const values = bindings.elements.filter((element) => !element.isTypeOnly);
      return {
        typeOnly: false,
        names: values.map((element) => (element.propertyName ?? element.name).text),
      };
    }
  }
  if (ts.isExportDeclaration(node)) {
    if (node.isTypeOnly) return { typeOnly: true, names: [] };
    if (node.exportClause && ts.isNamedExports(node.exportClause)) {
      const values = node.exportClause.elements.filter((element) => !element.isTypeOnly);
      return {
        typeOnly: false,
        names: values.map((element) => (element.propertyName ?? element.name).text),
      };
    }
  }
  return { typeOnly: false, names: null };
}
function forbiddenTarget(file, specifier, names) {
  if (specifier === '@proteus-int/intelligence')
    return (
      names === null || names.length === 0 || names.some((name) => !intelligenceSelectors.has(name))
    );
  if (specifier.startsWith('@tauri-apps/')) return true;
  if (
    specifier.startsWith('@proteus-int/') &&
    orchestrationPackages.has(specifier.slice(13).split('/')[0])
  )
    return true;
  if (!specifier.startsWith('.')) return false;
  const target = posix.normalize(posix.join(posix.dirname(file), specifier)).replace(/\.tsx?$/, '');
  const prefix = 'apps/desktop/src/shared/lib/';
  if (!target.startsWith(prefix)) return false;
  const module = target.slice(prefix.length);
  if (adapters.has(module)) return true;
  return (
    module === 'window-controls' &&
    (names === null || names.length === 0 || names.some((name) => !windowSelectors.has(name)))
  );
}
export function uiBoundaryErrors(source, file) {
  if (!/^apps\/[^/]+\/src\/(?:.*\/)?ui\//.test(file)) return [];
  const ast = parse(source, file);
  const errors = [];
  function check(node, specifier, binding = { typeOnly: false, names: null }) {
    if (!binding.typeOnly && forbiddenTarget(file, specifier, binding.names))
      errors.push(
        `${file}:${location(ast, node)}: UI must call its model instead of native/persistence/provider orchestration ${specifier}`,
      );
  }
  function visit(node) {
    if (
      (ts.isImportDeclaration(node) || ts.isExportDeclaration(node)) &&
      node.moduleSpecifier &&
      ts.isStringLiteral(node.moduleSpecifier)
    )
      check(node, node.moduleSpecifier.text, importBinding(node));
    if (
      ts.isImportEqualsDeclaration(node) &&
      ts.isExternalModuleReference(node.moduleReference) &&
      node.moduleReference.expression &&
      ts.isStringLiteral(node.moduleReference.expression)
    )
      check(node, node.moduleReference.expression.text, { typeOnly: node.isTypeOnly, names: null });
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
        check(node, argument.text);
    }
    ts.forEachChild(node, visit);
  }
  visit(ast);
  return errors;
}
