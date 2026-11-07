import { describe, expect, it } from 'vitest';
import invalidWire from './fixtures/invalid-ingestion-wire.json';
import {
  decodeIngestionBudget,
  decodeIngestionJob,
  decodeIngestionPreview,
  decodeIngestionSelection,
} from './ingestion-codec';
import type {
  IngestionBudget,
  IngestionJob,
  IngestionPreview,
  IngestionSelection,
} from './ingestion';

const exact = '  é 👩🏽‍🔬\r\nисточник\t';
const budget: IngestionBudget = {
  maxInputBytes: 128 * 1024 * 1024,
  maxExpandedBytes: 512 * 1024 * 1024,
  maxDocuments: 100000,
  maxBatchDocuments: 200,
  maxBatchBytes: 1024 * 1024,
  maxDurationMs: 60000,
};
const selection: IngestionSelection = {
  previewId: 'preview:1',
  resourceIds: ['chat:1'],
  bindings: [
    {
      schemaVersion: 1,
      id: 'binding:1',
      personaId: 'persona:a',
      provider: 'telegram',
      accountId: null,
      resourceId: 'chat:1',
      grantId: 'grant:1',
      policyVersion: 'policy:1',
      role: 'own',
      targetAuthorId: 'author:1',
      allowedContextAuthorIds: ['author:2'],
      allowUnknownContext: true,
      period: { from: null, to: null, includeUndated: true },
    },
  ],
};
const preview: IngestionPreview = {
  schemaVersion: 1,
  id: 'preview:1',
  sourceHandle: 'handle:opaque',
  sourceFormat: 'telegramJson',
  adapterVersion: 'telegram:1',
  resources: [
    { id: 'chat:1', label: exact, authorIds: ['author:1', 'author:2'], estimatedDocuments: null },
  ],
  limitations: [exact],
};
const job: IngestionJob = {
  schemaVersion: 1,
  id: 'job:1',
  selection,
  status: 'running',
  phase: 'reading',
  budget,
  checkpoint: {
    adapterVersion: 'telegram:1',
    cursor: exact,
    committedBatch: 2,
    stagingGeneration: 3,
  },
  coverage: {
    loadedDocuments: 400,
    indexedDocuments: 30,
    analyzedDocuments: 7,
    usedDocuments: 2,
    totalDocuments: null,
    loadedBytes: 20000,
    complete: false,
    limitations: [exact],
  },
  publishedSnapshotId: null,
  error: null,
};

describe('ingestion native boundary', () => {
  it('preserves exact labels, opaque cursors, unknown totals and distinct coverage', () => {
    expect(decodeIngestionPreview(preview)).toEqual(preview);
    expect(decodeIngestionJob(job)).toEqual(job);
    expect(decodeIngestionSelection(selection)).toEqual(selection);
    expect(decodeIngestionBudget(budget)).toEqual(budget);
    const failed = {
      ...job,
      status: 'failed',
      error: { code: 'readFailed', message: exact, retryable: true },
    };
    expect(decodeIngestionJob(failed)).toEqual(failed);
  });
  it('accepts completed ingestion before and after atomic snapshot publication', () => {
    const completed = {
      ...job,
      status: 'completed',
      phase: 'publishing',
      coverage: { ...job.coverage, complete: true, totalDocuments: 400 },
    };
    expect(decodeIngestionJob(completed)).toEqual(completed);
    expect(
      decodeIngestionJob({ ...completed, publishedSnapshotId: 'snapshot:1' }).publishedSnapshotId,
    ).toBe('snapshot:1');
  });
  it.each([
    { maxDocuments: 0 },
    { maxBatchBytes: -1 },
    { maxDurationMs: 1.5 },
    { maxInputBytes: Infinity },
    { maxExpandedBytes: Number.MAX_SAFE_INTEGER + 1 },
    { maxBatchDocuments: 100001 },
    { maxBatchBytes: 513 * 1024 * 1024 },
    invalidWire.budgetAlias,
  ])('rejects unsafe budgets and mixed legacy fields %j', (change) => {
    expect(() => decodeIngestionBudget({ ...budget, ...change })).toThrow();
  });
  it.each([
    { schemaVersion: 2 },
    { status: 'done' },
    { phase: 'unknown' },
    invalidWire.snapshotAlias,
    { error: { code: 'readFailed', message: exact, retryable: 'yes' } },
    { checkpoint: { ...job.checkpoint, committedBatch: -1 } },
    { checkpoint: { ...job.checkpoint, cursor: 12 } },
    { checkpoint: { ...job.checkpoint, extra: true } },
    { selection: { ...selection, resourceIds: ['unselected'] } },
  ])('rejects malformed jobs %j', (change) => {
    expect(() => decodeIngestionJob({ ...job, ...change })).toThrow();
  });
  it.each([
    { loadedDocuments: -1 },
    { indexedDocuments: 401 },
    { analyzedDocuments: 401 },
    { usedDocuments: 401 },
    { totalDocuments: 399 },
    { loadedBytes: -1 },
    { complete: 'yes' },
    { complete: true, totalDocuments: 401 },
    { limitations: [1] },
  ])('rejects contradictory coverage %j', (change) => {
    expect(() =>
      decodeIngestionJob({ ...job, coverage: { ...job.coverage, ...change } }),
    ).toThrow();
  });
  it.each(['queued', 'running', 'paused', 'cancelled', 'failed'])(
    'rejects published snapshots for %s jobs',
    (status) => {
      expect(() =>
        decodeIngestionJob({ ...job, status, publishedSnapshotId: 'snapshot:1' }),
      ).toThrow();
    },
  );
  it('rejects completed incomplete ingestion and failures without a reason', () => {
    expect(() => decodeIngestionJob({ ...job, status: 'completed' })).toThrow();
    expect(() => decodeIngestionJob({ ...job, status: 'failed' })).toThrow();
  });
  it.each([
    { resourceIds: [] },
    { resourceIds: ['chat:1', 'chat:1'] },
    { bindings: [] },
    { bindings: [selection.bindings[0], selection.bindings[0]] },
    { bindings: [{ ...selection.bindings[0], resourceId: 'foreign' }] },
    { bindings: [{ ...selection.bindings[0], targetAuthorId: null }] },
    { bindings: [{ ...selection.bindings[0], ...invalidWire.bindingAlias }] },
  ])('rejects invalid selections %j', (change) => {
    expect(() => decodeIngestionSelection({ ...selection, ...change })).toThrow();
  });
  it.each([
    { schemaVersion: 2 },
    { resources: [preview.resources[0], preview.resources[0]] },
    { resources: [{ ...preview.resources[0], estimatedDocuments: -1 }] },
    { resources: [{ ...preview.resources[0], authorIds: ['author:1', 'author:1'] }] },
    { resources: [{ ...preview.resources[0], extra: true }] },
  ])('rejects ambiguous previews %j', (change) => {
    expect(() => decodeIngestionPreview({ ...preview, ...change })).toThrow();
  });
});
