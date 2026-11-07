import { expect, it } from 'vitest';
import { decodeIngestionBindingRequest } from './ingestion-codec';
const request = {
  previewId: 'preview:1',
  resourceId: 'chat:1\r\nopaque',
  personaId: 'persona1',
  role: 'own',
  targetAuthorId: 'user1',
  allowedContextAuthorIds: ['user2'],
  allowUnknownContext: false,
  period: { from: null, to: null, includeUndated: true },
  allowModelProcessing: false,
  allowExport: false,
};
it('validates selection without granting model processing or export implicitly', () => {
  expect(decodeIngestionBindingRequest(request)).toEqual(request);
});
it('keeps model-processing and export permissions independent', () => {
  for (const allowModelProcessing of [true, false])
    for (const allowExport of [true, false]) {
      const value = { ...request, allowModelProcessing, allowExport };
      expect(decodeIngestionBindingRequest(value)).toEqual(value);
    }
});
it('accepts reference scope with unknown author and preserves source identifiers exactly', () => {
  const value = {
    ...request,
    role: 'reference',
    targetAuthorId: null,
    allowUnknownContext: true,
    allowedContextAuthorIds: ['  Автор é  '],
  };
  expect(decodeIngestionBindingRequest(value)).toEqual(value);
});
it.each([
  { previewId: '' },
  { resourceId: '  ' },
  { personaId: ' ' },
  { role: 'invented' },
  { targetAuthorId: null },
  { targetAuthorId: 'я'.repeat(65) },
  { allowedContextAuthorIds: ['user1'] },
  { allowedContextAuthorIds: ['user2', 'user2'] },
  { allowedContextAuthorIds: ['bad\nID'] },
  {
    allowedContextAuthorIds: Array.from({ length: 129 }, (_, i) => `user${i + 2}`),
  },
  { allowUnknownContext: 1 },
  { allowModelProcessing: undefined },
  { allowModelProcessing: 'yes' },
  { allowExport: 1 },
  { period: { from: null, to: null } },
  { period: { from: 3, to: null, includeUndated: true } },
  { grantId: 'forged' },
  { provider: 'telegram' },
  { accountId: 'forged' },
  { policyVersion: 'forged' },
  { sourceHandle: '/private/file' },
])('rejects malformed or caller-supplied authority before native dispatch (%#)', (change) => {
  expect(() => decodeIngestionBindingRequest({ ...request, ...change })).toThrow();
});
