import type * as SolidClient from 'solid-js';
import { createRoot, createSignal } from 'solid-js';
import { expect, it, vi } from 'vitest';
import type { SourceGrant } from '@proteus-int/contracts';
import { createSourceSelectionModel } from './source-selection-model';
import type { SourceSelectionPort } from './source-selection-model';

// Node otherwise selects Solid's SSR build, whose lifecycle effects are intentionally inert.
vi.mock('solid-js', () => vi.importActual<typeof SolidClient>('solid-js/dist/solid.js'));

const grant: SourceGrant = {
  id: 'grant-id',
  provider: 'telegram',
  resourceId: 'resource-id',
  authorId: null,
  from: null,
  to: null,
  accountId: 'account-id',
  snapshotId: 'snapshot-id',
  expiresAt: 'old-date',
  allowModelProcessing: true,
  operations: ['read'],
  budget: { maxSteps: 40, maxDocuments: 1000, maxBytes: 1048576, maxDurationMs: 300000 },
};
const fields = { author: '  author-id  ', from: '2026-01-01', to: '2026-11-07' };
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((complete) => {
    resolve = complete;
  });
  return { promise, resolve };
}
function setup(initialGrants: readonly SourceGrant[] = []) {
  const port = {
    importSource: vi.fn<SourceSelectionPort['importSource']>().mockResolvedValue(null),
    createSourceGrant: vi.fn<SourceSelectionPort['createSourceGrant']>().mockResolvedValue(grant),
    deleteSourceGrant: vi
      .fn<SourceSelectionPort['deleteSourceGrant']>()
      .mockResolvedValue(undefined),
  };
  const onImported = vi.fn();
  const onChange = vi.fn();
  let dispose!: () => void;
  const value = createRoot((stop) => {
    dispose = stop;
    const [grants, setGrants] = createSignal(initialGrants);
    const [available, setAvailable] = createSignal(true);
    const model = createSourceSelectionModel(
      {
        grants,
        modelProcessingAvailable: available,
        onImported,
        onChange: (grants) => {
          onChange(grants);
          setGrants(grants);
        },
      },
      port,
    );
    return { model, grants, setGrants, setAvailable };
  });
  return { ...value, dispose, port, onImported, onChange };
}

it('reads reactive grants and model availability without snapshotting component props', async () => {
  const fixture = setup();
  expect(fixture.model.adding()).toBe(true);
  fixture.setGrants([grant]);
  expect(fixture.model.current()).toEqual(grant);
  fixture.model.changeResource(' resource-id ');
  fixture.model.setConsent(true);
  fixture.setAvailable(false);
  await fixture.model.add(fields);
  expect(fixture.port.createSourceGrant).not.toHaveBeenCalled();
  fixture.setAvailable(true);
  await fixture.model.add(fields);
  expect(fixture.port.createSourceGrant).toHaveBeenCalledWith({
    provider: 'telegram',
    resourceId: 'resource-id',
    authorId: 'author-id',
    from: '2026-01-01T00:00:00.000Z',
    to: '2026-11-07T23:59:59.999Z',
    allowModelProcessing: true,
    operations: ['describe', 'search', 'collect', 'read', 'coverage'],
    budget: grant.budget,
  });
  expect(fixture.model.adding()).toBe(false);
  expect(fixture.model.consent()).toBe(false);
  expect(fixture.model.selectedGrant()).toBe(1);
  expect(fixture.grants()).toEqual([grant, grant]);
  fixture.dispose();
});

it('requires consent and preserves the five-grant maximum', async () => {
  const fixture = setup();
  await fixture.model.add(fields);
  expect(fixture.port.createSourceGrant).not.toHaveBeenCalled();
  fixture.model.setConsent(true);
  fixture.setGrants(Array.from({ length: 5 }, () => grant));
  await fixture.model.add(fields);
  expect(fixture.port.createSourceGrant).not.toHaveBeenCalled();
  fixture.dispose();
});

it('clears consent when a resource or provider changes and preserves empty dates as null', async () => {
  const fixture = setup();
  fixture.model.setConsent(true);
  fixture.model.changeResource('exact resource');
  expect(fixture.model.consent()).toBe(false);
  fixture.model.setConsent(true);
  fixture.model.selectProvider('github');
  expect(fixture.model.resource()).toBe('');
  expect(fixture.model.consent()).toBe(false);
  fixture.model.changeResource('owner/repo');
  fixture.model.setConsent(true);
  await fixture.model.add({ author: '  ', from: '', to: '' });
  expect(fixture.port.createSourceGrant).toHaveBeenCalledWith(
    expect.objectContaining({
      provider: 'github',
      resourceId: 'owner/repo',
      authorId: null,
      from: null,
      to: null,
    }),
  );
  fixture.dispose();
});

it('preserves import cancellation and hands exact metadata to the current callback', async () => {
  const fixture = setup();
  fixture.model.changeResource('original');
  await fixture.model.pick();
  expect(fixture.model.resource()).toBe('original');
  expect(fixture.onImported).not.toHaveBeenCalled();
  const imported = { id: 'import:source-id', label: 'Exact\r\n👩🏽‍🔬', documentCount: 3, warnings: [] };
  fixture.port.importSource.mockResolvedValue(imported);
  fixture.model.selectProvider('instagram');
  fixture.model.setConsent(true);
  await fixture.model.pick();
  expect(fixture.onImported).toHaveBeenCalledWith('instagram', imported);
  expect(fixture.model.resource()).toBe('import:source-id');
  expect(fixture.model.consent()).toBe(false);
  fixture.dispose();
});

it('keeps grant creation serial and discards a late grant after its owner is disposed', async () => {
  const fixture = setup();
  const pending = deferred<SourceGrant>();
  fixture.port.createSourceGrant.mockReturnValue(pending.promise);
  fixture.model.setConsent(true);
  const adding = fixture.model.add(fields);
  expect(fixture.model.busy()).toBe(true);
  await fixture.model.add(fields);
  expect(fixture.port.createSourceGrant).toHaveBeenCalledOnce();
  fixture.dispose();
  pending.resolve(grant);
  await adding;
  expect(fixture.onChange).not.toHaveBeenCalled();
});

it('does not deliver imported metadata after owner disposal', async () => {
  const fixture = setup();
  const pending = deferred<Awaited<ReturnType<SourceSelectionPort['importSource']>>>();
  fixture.port.importSource.mockReturnValue(pending.promise);
  const picking = fixture.model.pick();
  fixture.dispose();
  pending.resolve({ id: 'id', label: 'late', documentCount: 1, warnings: [] });
  await picking;
  expect(fixture.onImported).not.toHaveBeenCalled();
  expect(fixture.model.resource()).toBe('');
});

it('removes the selected grant through the port before applying the remaining list', async () => {
  const second = { ...grant, id: 'second' };
  const fixture = setup([grant, second]);
  fixture.model.setSelectedGrant(1);
  await fixture.model.remove();
  expect(fixture.port.deleteSourceGrant).toHaveBeenCalledWith('second');
  expect(fixture.grants()).toEqual([grant]);
  expect(fixture.model.current()).toEqual(grant);
  await fixture.model.remove();
  expect(fixture.model.adding()).toBe(true);
  expect(fixture.grants()).toEqual([]);
  fixture.dispose();
});

it('keeps existing grants when native deletion fails and reports a safe error', async () => {
  const fixture = setup([grant]);
  fixture.port.deleteSourceGrant.mockRejectedValue('native detail not for UI');
  await fixture.model.remove();
  expect(fixture.model.busy()).toBe(false);
  expect(fixture.grants()).toEqual([grant]);
  expect(fixture.model.error()).toBe('Не удалось отменить доступ. Повторите попытку.');
  fixture.dispose();
});
