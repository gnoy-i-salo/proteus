import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

test('window adapter fullscreen commands have explicit native permissions', () => {
  const { permissions } = JSON.parse(
    readFileSync(
      new URL('../apps/desktop/src-tauri/capabilities/main.json', import.meta.url),
      'utf8',
    ),
  );
  for (const command of ['is-fullscreen', 'set-fullscreen', 'toggle-maximize'])
    assert.ok(
      permissions.includes(`core:window:allow-${command}`),
      `Missing ${command} permission`,
    );
});
