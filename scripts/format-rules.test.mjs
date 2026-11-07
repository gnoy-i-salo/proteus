import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import prettier from 'prettier';
import ts from 'typescript';

const root = new URL('..', import.meta.url);
const options = JSON.parse(readFileSync(new URL('.prettierrc.json', root), 'utf8'));
function strings(source) {
  const values = [];
  const ast = ts.createSourceFile('payload.ts', source, ts.ScriptTarget.Latest, true);
  function visit(node) {
    if (ts.isStringLiteral(node) || ts.isNoSubstitutionTemplateLiteral(node))
      values.push(node.text);
    ts.forEachChild(node, visit);
  }
  visit(ast);
  return values;
}

test('formatter is idempotent and preserves literal Unicode, CRLF and external keys', async () => {
  const source =
    'export const payload={"source-url":"https://example.test/a_b", exactText:"  é 😀\\r\\nOriginal words  ", markdown:"first  \\nsecond"};\n';
  const formatted = await prettier.format(source, { ...options, parser: 'typescript' });
  assert.deepEqual(strings(formatted), strings(source));
  assert.equal(await prettier.format(formatted, { ...options, parser: 'typescript' }), formatted);
});

test('Markdown hard line breaks survive formatting and EditorConfig keeps semantic spaces', async () => {
  const markdown = 'First line  \nSecond line\n';
  assert.equal(await prettier.format(markdown, { ...options, parser: 'markdown' }), markdown);
  const editor = readFileSync(new URL('.editorconfig', root), 'utf8');
  const markdownRules = editor.match(/^\[\*\.md\]\r?\n([^[]*)/m)?.[1] ?? '';
  assert.match(markdownRules, /^trim_trailing_whitespace\s*=\s*false$/m);
  assert.match(editor, /^\[\*\.rs\]\r?\nindent_size\s*=\s*4$/m);
});

test('JSON and SCSS formatting is idempotent and preserves payload values', async () => {
  const json =
    '{"exactText":"  é 😀\\r\\nsource  ","external_key":"https://example.test/Exact-ID"}';
  const formattedJson = await prettier.format(json, { ...options, parser: 'json' });
  assert.deepEqual(JSON.parse(formattedJson), JSON.parse(json));
  assert.equal(await prettier.format(formattedJson, { ...options, parser: 'json' }), formattedJson);
  const scss =
    '.blockMore__text {--readingSize:14px;content:"  Exact-ID é 😀  ";font-size:var(--readingSize);}';
  const formattedScss = await prettier.format(scss, { ...options, parser: 'scss' });
  assert.ok(formattedScss.includes('  Exact-ID é 😀  '));
  assert.equal(await prettier.format(formattedScss, { ...options, parser: 'scss' }), formattedScss);
});
