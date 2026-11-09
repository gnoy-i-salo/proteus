import { describe, expect, it } from 'vitest';
import { decodeSourceExcerpt } from './source-excerpt-codec';
import { projectBoundExcerpt } from './source-excerpt';
import { binding, revision } from './testing/memory-fixtures';

const excerpt = () =>
  projectBoundExcerpt(binding, revision, {
    documentId: revision.document.id,
    revisionId: revision.revisionId,
    unit: 'utf8Bytes',
    start: 0,
    end: new TextEncoder().encode(revision.document.text).length,
  });
describe('source excerpt wire boundary', () => {
  it('retains exact source wording, attribution, relations and optional legacy metadata', () => {
    const input = excerpt();
    expect(decodeSourceExcerpt(input)).toEqual(input);
    const { sourceMetadata, ...legacy } = input;
    expect(sourceMetadata).toBeDefined();
    expect(decodeSourceExcerpt(legacy)).toEqual(legacy);
    const selected = '\uFEFF «Ёлка» 👩🏽‍🔬\r\n';
    const unicode = {
      ...input,
      text: selected,
      span: { ...input.span, start: 10, end: 10 + new TextEncoder().encode(selected).length },
    };
    expect(decodeSourceExcerpt(unicode).text).toBe(selected);
  });
  it.each(['web', 'savedPage'] as const)(
    'decodes %s metadata without inventing document text or grants',
    (provider) => {
      const base = excerpt();
      const input = { ...base, document: { ...base.document, provider } };
      const decoded = decodeSourceExcerpt(input);
      expect(decoded.document.provider).toBe(provider);
      expect(decoded.document).not.toHaveProperty('text');
      expect(decoded).not.toHaveProperty('grantId');
    },
  );
  it.each([
    { revisionId: 'duplicate' },
    { grantId: 'foreign' },
    { text: 42 },
    { text: '\ud800' },
    { role: 'admin' },
    { speakerRelation: 'author' },
    { canSupportOwnWords: 1 },
    { relations: null },
    { sourceMetadata: null },
  ])('rejects malformed or injected fields without changing inputs: %j', (patch) => {
    const input = { ...excerpt(), ...patch };
    const before = structuredClone(input);
    expect(() => decodeSourceExcerpt(input)).toThrow();
    expect(input).toEqual(before);
  });
  it('requires all non-optional fields and explicit nullable metadata', () => {
    for (const key of Object.keys(excerpt()).filter((key) => key !== 'sourceMetadata')) {
      const input: Record<string, unknown> = { ...excerpt() };
      delete input[key];
      expect(() => decodeSourceExcerpt(input)).toThrow();
    }
    const input = excerpt();
    expect(() =>
      decodeSourceExcerpt({
        ...input,
        document: { ...input.document, text: 'invented full document' },
      }),
    ).toThrow();
    const { authorId: _id, ...missing } = input.document;
    expect(_id).toBe('author-a');
    expect(() => decodeSourceExcerpt({ ...input, document: missing })).toThrow();
    expect(() =>
      decodeSourceExcerpt({
        ...input,
        sourceMetadata: { ...input.sourceMetadata, schemaVersion: 2 },
      }),
    ).toThrow();
  });
  it('rejects foreign identities, invalid byte geometry and held-out data', () => {
    const input = excerpt();
    for (const change of [
      { documentId: 'other' },
      { start: -1 },
      { end: input.span.end + 1 },
      { unit: 'utf16' },
      { end: Number.MAX_SAFE_INTEGER + 1 },
    ]) {
      expect(() => decodeSourceExcerpt({ ...input, span: { ...input.span, ...change } })).toThrow();
    }
    expect(() =>
      decodeSourceExcerpt({ ...input, document: { ...input.document, partition: 'holdout' } }),
    ).toThrow();
  });
  it('never promotes quoted, forwarded, unknown or context authorship into own words', () => {
    const input = excerpt();
    for (const patch of [
      { role: 'context' },
      { speakerRelation: 'unknown' },
      { document: { ...input.document, authorship: 'other' } },
      { document: { ...input.document, authorId: '  ' } },
      { document: { ...input.document, isForwarded: true } },
      { relations: [{ ...input.relations[0], kind: 'quote' }] },
    ]) {
      expect(() => decodeSourceExcerpt({ ...input, ...patch })).toThrow();
      expect(
        decodeSourceExcerpt({ ...input, ...patch, canSupportOwnWords: false }).canSupportOwnWords,
      ).toBe(false);
    }
  });
  it('validates relation fields without replacing missing original author labels', () => {
    const input = excerpt();
    const relation = {
      kind: 'forward',
      original: null,
      originalAuthor: { provider: null, id: null, label: '  Исходный автор\r\n' },
      span: null,
    };
    const decoded = decodeSourceExcerpt({
      ...input,
      canSupportOwnWords: false,
      relations: [relation],
    });
    expect(decoded.relations).toEqual([relation]);
    expect(() =>
      decodeSourceExcerpt({ ...input, relations: [{ ...relation, hidden: true }] }),
    ).toThrow();
    expect(() =>
      decodeSourceExcerpt({ ...input, relations: [{ ...relation, kind: 'citation' }] }),
    ).toThrow();
  });
});
