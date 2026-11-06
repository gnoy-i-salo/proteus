import { test } from 'node:test';
import assert from 'node:assert/strict';
import { validateRelease, requiredAssets } from './release.mjs';
test('release version must match manifests and a stable version tag', () => {
  assert.equal(validateRelease('v0.1.1', ['0.1.1', '0.1.1']), '0.1.1');
  assert.throws(() => validateRelease('v0.1.1', ['0.1.0']), /match/);
  assert.throws(() => validateRelease('trunk', ['0.1.1']), /tag/);
  assert.throws(() => validateRelease('v1.2.3;echo bad', ['1.2.3']), /tag/);
});
test('publication requires every expected platform artifact', () => {
  const assets = [
    'proteus-v0.1.1-macos-arm64.dmg',
    'proteus-v0.1.1-macos-x64.dmg',
    'proteus-v0.1.1-windows-x64.exe',
    'proteus-v0.1.1-linux-x64.deb',
  ];
  assert.doesNotThrow(() => requiredAssets('v0.1.1', assets));
  assert.throws(() => requiredAssets('v0.1.1', assets.slice(1)), /macos-arm64/);
});
