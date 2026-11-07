import assert from 'node:assert/strict';
import test from 'node:test';
import stylelint from 'stylelint';
import config from '../stylelint.config.mjs';

async function rules(code) {
  const result = await stylelint.lint({ code, codeFilename: 'naming-fixture.scss', config });
  return result.results.flatMap((result) => result.warnings.map((warning) => warning.rule));
}

for (const selector of [
  'blockMore',
  'blockMore__elementMore',
  'blockMore_modificatorMaybe',
  'blockMore__elementMore_modificatorMaybe',
]) {
  test(`accepts agreed BEM ${selector}`, async () => {
    assert.deepEqual(await rules(`.${selector} { color: #fff; }`), []);
  });
}
for (const selector of [
  'block-name',
  'block__element-name',
  'block--modifier',
  'block___element',
  'Block',
  'block__element_modifier-more',
]) {
  test(`rejects legacy or malformed BEM ${selector}`, async () => {
    assert.ok((await rules(`.${selector} { color: #fff; }`)).includes('selector-class-pattern'));
  });
}
test('accepts camelCase design tokens while retaining CSS and external attribute syntax', async () => {
  assert.deepEqual(
    await rules(
      '$readingSize: 14px;\n\n.blockMore[aria-selected="true"] {\n  --readingSize: 14px;\n\n  font-size: var(--readingSize);\n}',
    ),
    [],
  );
});
test('rejects kebab-case owned CSS and SCSS variables', async () => {
  const errors = await rules(
    '$reading-size: 14px;\n.block {\n  --reading-size: 14px;\n  font-size: var(--reading-size);\n}',
  );
  assert.ok(errors.includes('custom-property-pattern'));
  assert.ok(errors.includes('scss/dollar-variable-pattern'));
});
