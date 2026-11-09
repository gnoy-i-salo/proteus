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
