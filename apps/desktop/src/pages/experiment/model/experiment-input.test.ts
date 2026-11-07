import type * as SolidClient from 'solid-js';
import { createRoot } from 'solid-js';
import { expect, it, vi } from 'vitest';
import type { SourceGrant } from '@proteus-int/contracts';
import { createModelSettings, setComponentModel, setModelAlias } from '@proteus-int/intelligence';
import { modelSettingsKeys } from '../../../shared/lib/model-settings-storage';
import { createExperimentInput } from './experiment-input';
import { createExperimentSettings } from './experiment-settings';
import { createExperimentSources } from './experiment-sources';
vi.mock('solid-js', () => vi.importActual<typeof SolidClient>('solid-js/dist/solid.js'));

function owned<T>(create: () => T) {
  let dispose!: () => void;
  const model = createRoot((stop) => {
    dispose = stop;
    return create();
  });
  return { model, dispose };
}
function storage() {
  const values = new Map<string, string>();
  return {
    values,
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => {
      values.set(key, value);
    },
    removeItem: (key: string) => {
      values.delete(key);
    },
  };
}
const grant: SourceGrant = {
  provider: 'telegram',
  resourceId: 'source-id',
  authorId: null,
  from: null,
  to: null,
  allowModelProcessing: false,
  operations: ['read'],
  budget: { maxSteps: 1, maxDocuments: 1, maxBytes: 100, maxDurationMs: 100 },
  id: 'grant-id',
  accountId: 'account-id',
  snapshotId: 'snapshot-id',
  expiresAt: 'old-date',
};

it('joins input pages exactly and only normalizes the existing public URL fields', () => {
  const { model, dispose } = owned(createExperimentInput);
  model.setNames(['  Анна ', 'Борис']);
  model.setContexts([['  A\r\n', '👩🏽‍🔬 '], ['B']]);
  model.setTask([' first\n', ' second ']);
  model.setUrls([[' https://example.test/source-url ', ''], ['']]);
  expect(model.currentInput()).toEqual({
    task: ' first\n second ',
    people: [
      { label: '  Анна ', context: '  A\r\n👩🏽‍🔬 ', publicUrls: ['https://example.test/source-url'] },
      { label: 'Борис', context: 'B', publicUrls: [] },
    ],
  });
  model.setRepetitions(3);
  const input = model.currentInput();
  model.applyInput(input);
  expect(model.currentInput()).toEqual(input);
  expect(model.urls()[1]).toEqual(['']);
  expect(model.revision()).toBe(1);
  expect(model.repetitions()).toBe(3);
  dispose();
});

it('keeps source identity and removes cleared-provider attachments without duplicate native revocations', () => {
  const remove = vi.fn().mockResolvedValue(undefined);
  const { model, dispose } = owned(() => createExperimentSources(remove, vi.fn()));
  const other = { ...grant, id: 'github-grant', provider: 'github' as const };
  model.changeGrants(0, [grant, other]);
  model.changeGrants(1, [grant]);
  model.rememberImport('telegram', {
    id: 'import:id',
    label: ' exact\r\n',
    documentCount: 1,
    warnings: [],
  });
  model.rememberImport('telegram', {
    id: 'import:id',
    label: ' updated\r\n',
    documentCount: 2,
    warnings: [],
  });
  expect(model.sourceImports()).toHaveLength(1);
  expect(model.sourceImports()[0]?.source.label).toBe(' updated\r\n');
  model.clearProvider('telegram');
  expect(model.sourceGrants()).toEqual([[other], []]);
  expect(model.sourceImports()).toEqual([]);
  expect(remove).not.toHaveBeenCalled();
  model.clearSourceGrants();
  expect(remove).toHaveBeenCalledExactlyOnceWith('github-grant');
  expect(model.sourceGrants()).toEqual([[], []]);
  dispose();
});

it('preserves unreadable settings and applies defaults only in memory', () => {
  const local = storage();
  local.values.set(modelSettingsKeys.legacy, '{broken exact');
  const reportError = vi.fn();
  const { model, dispose } = owned(() =>
    createExperimentSettings({ reportError, clearSourceGrants: vi.fn() }, () => local),
  );
  expect(model.settings()).toEqual(createModelSettings());
  expect(local.values.get(modelSettingsKeys.legacy)).toBe('{broken exact');
  expect(local.values.has(modelSettingsKeys.current)).toBe(false);
  expect(reportError).toHaveBeenCalledWith(expect.stringContaining('Исходная запись сохранена'));
  dispose();
});

it('applies model changes despite failed persistence and only invalidates STALKER-bound grants', () => {
  const local = storage();
  const clearSourceGrants = vi.fn();
  const reportError = vi.fn();
  const { model, dispose } = owned(() =>
    createExperimentSettings({ reportError, clearSourceGrants }, () => local),
  );
  const otherChange = setComponentModel(model.settings(), 'person', 'kimi');
  model.persist(otherChange);
  expect(clearSourceGrants).not.toHaveBeenCalled();
  expect(model.sourceModelsAvailable()).toBe(false);
  expect(JSON.parse(local.values.get(modelSettingsKeys.current) ?? '{}')).toHaveProperty(
    'version',
    2,
  );
  vi.spyOn(local, 'setItem').mockImplementation(() => {
    throw new Error('quota');
  });
  const changed = setModelAlias(model.settings(), 'stalker', 'exact alias');
  model.persist(changed);
  expect(model.settings()).toEqual(changed);
  expect(clearSourceGrants).toHaveBeenCalledOnce();
  expect(reportError).toHaveBeenLastCalledWith('Настройки применены, но не сохранены.');
  dispose();
});

it('reports a failed native permission revocation after detaching the source from input', async () => {
  const remove = vi
    .fn<(id: string) => Promise<void>>()
    .mockRejectedValue(new Error('native failure'));
  const reportError = vi.fn();
  const { model, dispose } = owned(() => createExperimentSources(remove, reportError));
  model.changeGrants(0, [grant]);
  model.clearSourceGrants();
  await Promise.resolve();
  expect(model.sourceGrants()).toEqual([[], []]);
  expect(reportError).toHaveBeenCalledWith(
    expect.stringContaining('не удалось подтвердить отзыв разрешений'),
  );
  dispose();
});
