import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
const workflow = readFileSync('.github/workflows/release.yml', 'utf8');
function verifyWorkflow(text) {
  const quality = text.split('  quality:\n')[1]?.split('  build:\n')[0] ?? '';
  const build = text.split('  build:\n')[1]?.split('  publish:\n')[0] ?? '';
  const publish = text.split('  publish:\n')[1] ?? '';
  assert.match(quality, /sourceSha: \$\{\{ steps\.source\.outputs\.sourceSha \}\}/);
  assert.match(quality, /- id: source\s+run: node scripts\/release\.mjs source/);
  for (const job of [build, publish]) {
    assert.match(job, /^ {10}ref: \$\{\{ needs\.quality\.outputs\.sourceSha \}\}$/m);
    assert.match(job, /^ {6}SOURCE_SHA: \$\{\{ needs\.quality\.outputs\.sourceSha \}\}$/m);
  }
  assert.match(quality, /^ {6}- run: pnpm build:pages$/m);
  assert.match(build, /^ {4}needs: quality$/m);
  assert.match(publish, /^ {4}needs: \[quality, build\]$/m);
  assert.match(publish, /gh release create .*--verify-tag --draft/);
  assert.match(publish, /if \[ "\$\(cat \/tmp\/is-draft\)" != true \]; then[\s\S]*?exit 1/);
  assert.doesNotMatch(publish, /gh release (?:edit|publish)/);
  assert.match(publish, /gh release upload .*release-assets\/BUILD-MANIFEST\.json --clobber/);
  assert.doesNotMatch(publish, /gh release upload .*release-assets\/\* --clobber/);
}
test('release builds and draft consume the immutable quality SHA and preserve draft-only publishing', () =>
  verifyWorkflow(workflow));
test('workflow regression fixtures reject tag checkouts, missing provenance and published-asset mutation', () => {
  const valid = workflow;
  for (const [before, after] of [
    ['ref: ${{ needs.quality.outputs.sourceSha }}', 'ref: ${{ github.ref }}'],
    ['SOURCE_SHA: ${{ needs.quality.outputs.sourceSha }}', 'SOURCE_SHA: ${{ github.sha }}'],
    ['needs: [quality, build]', 'needs: build'],
    ['- run: pnpm build:pages', '- run: pnpm build'],
    ['--verify-tag --draft', '--verify-tag'],
    ['"$(cat /tmp/is-draft)" != true', '"$(cat /tmp/is-draft)" = false'],
    ['release-assets/BUILD-MANIFEST.json --clobber', 'release-assets/* --clobber'],
  ]) {
    assert(valid.includes(before), `fixture target missing: ${before}`);
    assert.throws(() => verifyWorkflow(valid.replace(before, after)), after);
  }
});
