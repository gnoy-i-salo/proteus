import { describe, expect, it } from 'vitest';
import {
  decodeLegacySourceDocument,
  decodeStoredSourceDocument,
  encodeStoredSourceDocument,
} from './source-document-codec';

const legacy = {
  id: 'message-17',
  provider: 'telegram',
  'resource-id': 'chat-1',
  'author-id': null,
  authorship: 'unknown',
  'author-label': 'Анна (@anna)',
  kind: 'message',
  mode: 'dialogue',
  text: '  «Борис: ок»\r\n# source-url\n👩🏽‍💻\t ',
  'source-url': 'https://example.org/a-b',
  'created-at': null,
  'retrieved-at': '2026-11-07T12:00:00Z',
  'content-hash': 'unchanged-hash',
  'reply-to': 'message-16',
  'is-forwarded': true,
  truncated: false,
  partition: 'reference',
};

describe('legacy source document boundary', () => {
  it('round trips versioned documents without changing held-out content or relationships', () => {
    const document = decodeLegacySourceDocument({
      ...legacy,
      partition: 'holdout',
      authorship: 'other',
    });
    const encoded = encodeStoredSourceDocument(document);
    expect(decodeStoredSourceDocument(encoded)).toEqual(document);
    expect(JSON.parse(encoded)).toMatchObject({ format: 'proteus-source-document', version: 2 });
    expect(encodeStoredSourceDocument(decodeStoredSourceDocument(encoded))).toBe(encoded);
    expect(legacy.partition).toBe('reference');
  });

  it.each([0, 1, 3, '2', null])('rejects unsupported version %j', (version) => {
    expect(() =>
      decodeStoredSourceDocument(
        JSON.stringify({
          format: 'proteus-source-document',
          version,
          document: decodeLegacySourceDocument(legacy),
        }),
      ),
    ).toThrow();
  });

  it('rejects legacy fields inside a new document, malformed envelopes and oversized JSON', () => {
    const document = decodeLegacySourceDocument(legacy);
    for (const input of [
      null,
      {},
      [],
      { format: 'other', version: 2, document },
      {
        format: 'proteus-source-document',
        version: 2,
        document: { ...document, 'source-url': legacy['source-url'] },
      },
      { format: 'proteus-source-document', version: 2, document, grant: 'not-permitted' },
    ])
      expect(() => decodeStoredSourceDocument(JSON.stringify(input))).toThrow();
    expect(() => decodeStoredSourceDocument('{')).toThrow();
    expect(() => decodeStoredSourceDocument(' '.repeat(32 * 1024 * 1024 + 1))).toThrow(/32/);
  });
  it('preserves unknown identity, relationships, URL and every source character', () => {
    expect(decodeLegacySourceDocument(legacy)).toEqual({
      id: 'message-17',
      provider: 'telegram',
      resourceId: 'chat-1',
      authorId: null,
      authorship: 'unknown',
      authorLabel: 'Анна (@anna)',
      kind: 'message',
      mode: 'dialogue',
      text: legacy.text,
      sourceUrl: legacy['source-url'],
      createdAt: null,
      retrievedAt: legacy['retrieved-at'],
      contentHash: 'unchanged-hash',
      replyTo: 'message-16',
      isForwarded: true,
      truncated: false,
      partition: 'reference',
    });
  });

  it.each([
    { ...legacy, resourceId: 'different-chat' },
    { ...legacy, resourceId: 'chat-1' },
    { ...legacy, 'author-id': 1 },
    { ...legacy, authorship: 'inferred' },
    { ...legacy, text: null },
    { ...legacy, partition: 'training' },
    { ...legacy, provider: 'unknown' },
    { ...legacy, 'is-forwarded': 'true' },
    { ...legacy, 'created-at': undefined },
  ])('rejects invalid or mixed input without repairing source data: %j', (value) => {
    expect(() => decodeLegacySourceDocument(value)).toThrow();
  });
});
