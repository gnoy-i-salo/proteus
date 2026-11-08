import { expect, it } from 'vitest';
import fixture from './fixtures/local-model-wire.json';
import {
  decodeLocalModelState,
  decodeLocalModelProgress,
  decodeLocalModelPackageState,
} from './local-model-wire';
it('accepts the shared native fixture without conflating cached files with a loaded runtime', () => {
  expect(decodeLocalModelState(fixture.state)).toEqual(fixture.state);
  expect(decodeLocalModelProgress(fixture.progress, 'download-fixture')).toEqual(fixture.progress);
  expect(decodeLocalModelPackageState('ready')).toBe('ready');
  expect(decodeLocalModelPackageState('missing')).toBe('missing');
  expect(fixture.state.runtimeLoaded).toBe(false);
});
it('rejects unknown schema versions, aliases, missing fields and native paths', () => {
  for (const change of [
    { schemaVersion: 2 },
    { schemaVersion: undefined },
    { cacheDir: '/private/cache' },
    Object.fromEntries([['model-id', fixture.state.modelId]]),
    { runtimeLoaded: 1 },
    { operationActive: undefined },
    { modelId: 'untrusted/model' },
    { modelRevision: 'branch-main' },
  ]) {
    expect(() => decodeLocalModelState({ ...fixture.state, ...change })).toThrow();
  }
  for (const value of [null, [], true, 'installed']) {
    expect(() => decodeLocalModelState(value)).toThrow();
    expect(() => decodeLocalModelPackageState(value)).toThrow();
  }
});
it('bounds package and progress counters and keeps operation identity exact', () => {
  for (const packageBytes of [0, -1, 1.5, Number.NaN, Number.POSITIVE_INFINITY, 1024 ** 3 + 1])
    expect(() => decodeLocalModelState({ ...fixture.state, packageBytes })).toThrow();
  expect(() => decodeLocalModelProgress(fixture.progress, 'other')).toThrow();
  for (const change of [
    { schemaVersion: 2 },
    { operationId: ' download-fixture' },
    { artifactsVerified: 4 },
    { artifactsVerified: 1.5 },
    { bytesVerified: -1 },
    { bytesVerified: 1024 ** 3 + 1 },
    { bytesVerified: 0 },
    { artifactsVerified: 0 },
    Object.fromEntries([['bytes_verified', 10]]),
  ])
    expect(() =>
      decodeLocalModelProgress({ ...fixture.progress, ...change }, 'download-fixture'),
    ).toThrow();
  expect(
    decodeLocalModelProgress(
      { ...fixture.progress, artifactsVerified: 0, bytesVerified: 0 },
      'download-fixture',
    ).artifactsVerified,
  ).toBe(0);
});
