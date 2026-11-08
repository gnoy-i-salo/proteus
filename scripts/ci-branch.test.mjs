import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
function verifyBranchCI(workflow) {
  const push = workflow.split('  push:\n')[1]?.split('  pull_request:\n')[0] ?? '';
  assert.match(push, /branches: \[trunk, 'codex\/\*\*'\]/);
  const deploy = workflow.split('  pages-deploy:\n')[1] ?? '';
  assert.match(deploy, /needs: \[quality, native, pages-build\]/);
  assert.match(
    deploy,
    /if: github.ref == 'refs\/heads\/trunk' && github.event_name != 'pull_request'/,
  );
}
const workflow = readFileSync('.github/workflows/ci.yml', 'utf8');
test('working branches run the full CI without publishing Pages', () => verifyBranchCI(workflow));
test('missing working-branch checks or public branch deployment are rejected', () => {
  const valid = workflow.replace(
    '  push:\n    branches: [trunk]',
    "  push:\n    branches: [trunk, 'codex/**']",
  );
  verifyBranchCI(valid);
  assert.throws(() => verifyBranchCI(valid.replace(", 'codex/**'", '')));
  assert.throws(() =>
    verifyBranchCI(valid.replace("\n    if: github.ref == 'refs/heads/trunk' &&", '\n    if:')),
  );
  assert.throws(() =>
    verifyBranchCI(valid.replace('needs: [quality, native, pages-build]', 'needs: quality')),
  );
});
