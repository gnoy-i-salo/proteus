import { expect, it } from 'vitest';
import v4 from '../testing/legacy-results/v4-current-with-execution.json';
import { decodeV4Result } from './result-v4';
import { prepareStoredResult } from './prepare-stored-result';

it('uses the existing export policy for private archived content without overwriting the stored snapshot', () => {
  const result = decodeV4Result(v4);
  const privateText = 'Exact private archived source text, long enough for the export filter.\r\n';
  const privateSample = {
    id: 'private',
    documentId: 'doc-private',
    mode: 'dialogue',
    text: privateText,
    context: 'private context',
    redactions: [],
    exportAllowed: false,
  };
  Object.assign(result.people[0]!.profile, {
    styleExemplars: [privateSample],
    sourceContext: true,
  });
  Object.assign(result.people[0]!.report!.evidence[0]!, {
    verification: 'source-retrieved',
    excerpt: privateText,
  });
  Object.assign(result.traces[0]!.trace, { summary: privateText });
  const before = JSON.stringify(result);
  const prepared = prepareStoredResult({ sourceVersion: 4, result });
  expect(JSON.stringify(prepared.view)).not.toContain(JSON.stringify(privateText).slice(1, -1));
  expect(prepared.stored.result).toBe(result);
  expect(JSON.stringify(prepared.stored.result)).toBe(before);
  expect(prepared.view.result.people[0].score).toEqual(result.people[0]!.score);
});

it('applies the same source policy to a legacy view without upgrading its protocol or changing its original', async () => {
  const { default: raw } = await import('../testing/legacy-results/v3-kebab-export.json');
  const { readStoredResult } = await import('./read-stored-result');
  const stored = readStoredResult(JSON.stringify(raw));
  if (stored.sourceVersion !== 3) throw new Error('Expected v3 fixture');
  const result = stored.result;
  const privateText = 'Private archived wording with CRLF and имя «Лира».\r\n';
  Object.assign(result.people[0].profile, {
    styleExemplars: [
      {
        id: 'private',
        documentId: 'doc-private',
        mode: 'dialogue',
        text: privateText,
        context: 'Private context',
        redactions: [],
        exportAllowed: false,
      },
    ],
    sourceContext: true,
  });
  Object.assign(result.people[0].report!.evidence[0]!, {
    verification: 'source-retrieved',
    excerpt: privateText,
  });
  Object.assign(result.traces[0]!.trace, { summary: privateText });
  const before = JSON.stringify(result);
  const prepared = prepareStoredResult(stored);
  expect(JSON.stringify(prepared.view)).not.toContain(JSON.stringify(privateText).slice(1, -1));
  expect(prepared.view.sourceVersion).toBe(3);
  expect(prepared.view.result).toMatchObject({ schemaVersion: 3, protocol: { version: 2 } });
  expect(prepared.view.result.people[0].score).toEqual(result.people[0].score);
  expect(JSON.stringify(prepared.stored.result)).toBe(before);
});

it('does not invent historical export metadata or replace the original export counts in a legacy view', async () => {
  const { readStoredResult } = await import('./read-stored-result');
  for (const raw of [
    (await import('../testing/legacy-results/v2-kebab-result.json')).default,
    (await import('../testing/legacy-results/v3-kebab-export.json')).default,
  ]) {
    const stored = readStoredResult(JSON.stringify(raw));
    if (stored.sourceVersion !== 2 && stored.sourceVersion !== 3) throw new Error('Legacy fixture');
    const view = prepareStoredResult(stored).view;
    if (view.sourceVersion !== 2 && view.sourceVersion !== 3) throw new Error('Legacy view');
    expect(view.result.exportMetadata).toEqual(stored.result.exportMetadata);
    expect(Object.hasOwn(view.result, 'exportMetadata')).toBe(
      Object.hasOwn(stored.result, 'exportMetadata'),
    );
  }
});
