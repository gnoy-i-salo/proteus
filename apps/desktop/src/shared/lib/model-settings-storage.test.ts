import { expect, it, vi } from 'vitest';
import { decodeModelSettings, encodeModelSettings } from '@proteus-int/intelligence';
import { migrateModelSettings, modelSettingsKeys } from './model-settings-storage';

const raw = JSON.stringify({
  'global-model': 'codex',
  components: {
    stalker: 'codex',
    'person-fabric': 'kimi',
    person: 'codex',
    'brain-analyzator': 'kimi',
  },
  aliases: {
    stalker: '  Модель\n🧠  ',
    'person-fabric': '',
    person: '',
    'brain-analyzator': '',
  },
});

class MemoryStorage {
  readonly values = new Map<string, string>();
  readonly writes: string[] = [];
  failAt: string | undefined;
  corruptPending = false;
  failCleanup = false;
  corruptCurrent = false;
  failCurrentReadOnce = false;
  currentWritten = false;
  concurrentCurrent: string | undefined;
  changeLegacyAfterStage: string | undefined;
  getItem(key: string) {
    if (key === modelSettingsKeys.current && this.currentWritten && this.failCurrentReadOnce) {
      this.failCurrentReadOnce = false;
      throw new Error('Read unavailable');
    }
    return this.values.get(key) ?? null;
  }
  setItem(key: string, value: string) {
    if (this.failAt === key) throw new Error('Disk full');
    this.writes.push(key);
    this.values.set(
      key,
      (this.corruptPending && key === modelSettingsKeys.pending) ||
        (this.corruptCurrent && key === modelSettingsKeys.current)
        ? '{}'
        : value,
    );
    if (key === modelSettingsKeys.current) {
      this.currentWritten = true;
      if (this.concurrentCurrent !== undefined) this.values.set(key, this.concurrentCurrent);
    }
    if (key === modelSettingsKeys.pending && this.changeLegacyAfterStage !== undefined)
      this.values.set(modelSettingsKeys.legacy, this.changeLegacyAfterStage);
  }
  removeItem(key: string) {
    if (this.failCleanup) throw new Error('Cleanup unavailable');
    this.values.delete(key);
  }
}

function legacyStorage() {
  const storage = new MemoryStorage();
  storage.values.set(modelSettingsKeys.legacy, raw);
  return storage;
}

it('verifies a staged version before publishing and retains the exact original', () => {
  const storage = legacyStorage();
  expect(migrateModelSettings(storage)).toEqual({
    settings: decodeModelSettings(raw),
    migrated: true,
    cleanupPending: false,
  });
  expect(storage.writes).toEqual([
    modelSettingsKeys.legacySnapshot,
    modelSettingsKeys.pending,
    modelSettingsKeys.current,
  ]);
  expect(storage.getItem(modelSettingsKeys.legacy)).toBe(raw);
  expect(storage.getItem(modelSettingsKeys.current)).toBe(
    encodeModelSettings(decodeModelSettings(raw)),
  );
  expect(storage.getItem(modelSettingsKeys.pending)).toBeNull();
  storage.writes.length = 0;
  expect(migrateModelSettings(storage)?.migrated).toBe(false);
  expect(storage.writes).toEqual([]);
});

it.each([modelSettingsKeys.legacySnapshot, modelSettingsKeys.pending, modelSettingsKeys.current])(
  'recovers after a failed write at %s without source loss',
  (key) => {
    const storage = legacyStorage();
    storage.failAt = key;
    expect(() => migrateModelSettings(storage)).toThrow('Disk full');
    expect(storage.getItem(modelSettingsKeys.legacy)).toBe(raw);
    expect(storage.getItem(modelSettingsKeys.current)).toBeNull();
    storage.failAt = undefined;
    expect(migrateModelSettings(storage)?.settings).toEqual(decodeModelSettings(raw));
  },
);

it('never publishes an unverified staging value and can restart from the original', () => {
  const storage = legacyStorage();
  storage.corruptPending = true;
  expect(() => migrateModelSettings(storage)).toThrow();
  expect(storage.getItem(modelSettingsKeys.current)).toBeNull();
  expect(storage.getItem(modelSettingsKeys.legacy)).toBe(raw);
  storage.corruptPending = false;
  expect(migrateModelSettings(storage)?.settings).toEqual(decodeModelSettings(raw));
});

it('does not turn missing or corrupted data into a persisted default', () => {
  const storage = new MemoryStorage();
  expect(migrateModelSettings(storage)).toBeNull();
  expect(storage.writes).toEqual([]);
  storage.values.set(modelSettingsKeys.legacy, '{}');
  expect(() => migrateModelSettings(storage)).toThrow();
  expect(storage.writes).toEqual([]);
});

it('rejects a corrupt current version without silently restoring stale legacy data', () => {
  const storage = legacyStorage();
  storage.values.set(modelSettingsKeys.current, '{"format":"proteus-model-settings","version":99}');
  expect(() => migrateModelSettings(storage)).toThrow();
  expect(storage.writes).toEqual([]);
  expect(storage.getItem(modelSettingsKeys.legacy)).toBe(raw);
});

it('reports pending cleanup without discarding a verified committed value', () => {
  const storage = legacyStorage();
  storage.failCleanup = true;
  expect(migrateModelSettings(storage)).toEqual({
    settings: decodeModelSettings(raw),
    migrated: true,
    cleanupPending: true,
  });
  storage.failCleanup = false;
  expect(migrateModelSettings(storage)?.cleanupPending).toBe(false);
  expect(storage.getItem(modelSettingsKeys.pending)).toBeNull();
});

it('quarantines our malformed publication before rollback and retries from the exact original', () => {
  const storage = legacyStorage();
  storage.corruptCurrent = true;
  expect(() => migrateModelSettings(storage)).toThrow();
  expect(storage.getItem(modelSettingsKeys.current)).toBeNull();
  expect(storage.getItem(modelSettingsKeys.recovery)).toBe('{}');
  expect(storage.getItem(modelSettingsKeys.legacy)).toBe(raw);
  storage.corruptCurrent = false;
  expect(migrateModelSettings(storage)?.settings).toEqual(decodeModelSettings(raw));
  expect(storage.getItem(modelSettingsKeys.recovery)).toBe('{}');
});

it.each([false, true])(
  'recovers after publication readback fails, including malformed output %s',
  (corrupt) => {
    const storage = legacyStorage();
    storage.corruptCurrent = corrupt;
    storage.failCurrentReadOnce = true;
    expect(() => migrateModelSettings(storage)).toThrow('Read unavailable');
    expect(storage.getItem(modelSettingsKeys.legacy)).toBe(raw);
    storage.corruptCurrent = false;
    expect(migrateModelSettings(storage)?.settings).toEqual(decodeModelSettings(raw));
    expect(storage.getItem(modelSettingsKeys.recovery)).toBe(corrupt ? '{}' : null);
  },
);

it('does not delete malformed current if quarantine cannot be durably verified', () => {
  const storage = legacyStorage();
  storage.corruptCurrent = true;
  storage.failAt = modelSettingsKeys.recovery;
  expect(() => migrateModelSettings(storage)).toThrow();
  expect(storage.getItem(modelSettingsKeys.current)).toBe('{}');
  expect(storage.getItem(modelSettingsKeys.legacy)).toBe(raw);
  storage.failAt = undefined;
  storage.corruptCurrent = false;
  expect(migrateModelSettings(storage)?.settings).toEqual(decodeModelSettings(raw));
  expect(storage.getItem(modelSettingsKeys.recovery)).toBe('{}');
});

it('preserves an unrelated valid current from another writer on readback', () => {
  const storage = legacyStorage();
  const other = { ...decodeModelSettings(raw), globalModel: 'kimi' as const };
  const encoded = encodeModelSettings(other);
  storage.concurrentCurrent = encoded;
  expect(() => migrateModelSettings(storage)).toThrow();
  expect(storage.getItem(modelSettingsKeys.current)).toBe(encoded);
  expect(storage.getItem(modelSettingsKeys.recovery)).toBeNull();
  expect(migrateModelSettings(storage)?.settings).toEqual(other);
});

it('refuses recovery without a matching pending value and exact unchanged legacy snapshot', () => {
  for (const damage of [
    'missingSnapshot',
    'changedLegacy',
    'changedPending',
    'differentRecovery',
  ]) {
    const storage = legacyStorage();
    storage.values.set(modelSettingsKeys.current, '{}');
    storage.values.set(modelSettingsKeys.pending, encodeModelSettings(decodeModelSettings(raw)));
    storage.values.set(modelSettingsKeys.legacySnapshot, raw);
    if (damage === 'missingSnapshot') storage.values.delete(modelSettingsKeys.legacySnapshot);
    if (damage === 'changedLegacy') storage.values.set(modelSettingsKeys.legacy, raw + ' ');
    if (damage === 'changedPending') storage.values.set(modelSettingsKeys.pending, '{}');
    if (damage === 'differentRecovery')
      storage.values.set(modelSettingsKeys.recovery, '{"unrelated":true}');
    expect(() => migrateModelSettings(storage)).toThrow();
    expect(storage.getItem(modelSettingsKeys.current)).toBe('{}');
  }
});

it('does not publish stale settings if legacy changes after staging', () => {
  const storage = legacyStorage();
  storage.changeLegacyAfterStage = raw + ' ';
  expect(() => migrateModelSettings(storage)).toThrow();
  expect(storage.getItem(modelSettingsKeys.current)).toBeNull();
  expect(storage.getItem(modelSettingsKeys.legacy)).toBe(raw + ' ');
});

it('rejects oversized current before attempting to parse it', () => {
  const storage = legacyStorage();
  const oversized = ' '.repeat(16385);
  storage.values.set(modelSettingsKeys.current, oversized);
  const parse = vi.spyOn(JSON, 'parse');
  try {
    expect(() => migrateModelSettings(storage)).toThrow();
    expect(parse).not.toHaveBeenCalledWith(oversized);
    expect(storage.writes).toEqual([]);
  } finally {
    parse.mockRestore();
  }
});

it('never treats an unsupported version as recoverable corruption, even with our staging witness', () => {
  const storage = legacyStorage();
  const future = '{"format":"proteus-model-settings","version":99,"futureData":"keep"}';
  storage.values.set(modelSettingsKeys.current, future);
  storage.values.set(modelSettingsKeys.pending, encodeModelSettings(decodeModelSettings(raw)));
  storage.values.set(modelSettingsKeys.legacySnapshot, raw);
  expect(() => migrateModelSettings(storage)).toThrow();
  expect(storage.getItem(modelSettingsKeys.current)).toBe(future);
  expect(storage.writes).toEqual([]);
});

it('retains quarantine and original if removal fails, then completes on restart', () => {
  const storage = legacyStorage();
  storage.corruptCurrent = true;
  storage.failCleanup = true;
  expect(() => migrateModelSettings(storage)).toThrow('Cleanup unavailable');
  expect(storage.getItem(modelSettingsKeys.current)).toBe('{}');
  expect(storage.getItem(modelSettingsKeys.recovery)).toBe('{}');
  storage.corruptCurrent = false;
  storage.failCleanup = false;
  expect(migrateModelSettings(storage)?.settings).toEqual(decodeModelSettings(raw));
  expect(storage.getItem(modelSettingsKeys.legacy)).toBe(raw);
});
