import { describe, expect, it } from 'vitest';
import { projectBoundExcerpt } from './source-excerpt';
import type { SourceSpan } from './corpus';
import { binding, revision } from './testing/memory-fixtures';

const encoder = new TextEncoder();
const span: SourceSpan = {
  documentId: revision.document.id,
  revisionId: revision.revisionId,
  unit: 'utf8Bytes',
  start: 0,
  end: encoder.encode(revision.document.text).length,
};

describe('bounded source excerpt projection', () => {
  it('preserves exact full Unicode/CRLF/source identity while omitting full document text and permissions', () => {
    const value = projectBoundExcerpt(binding, revision, span);
    const { text, ...metadata } = revision.document;
    expect(value).toEqual({
      span,
      text,
      document: metadata,
      role: 'own',
      speakerRelation: 'target',
      canSupportOwnWords: true,
      relations: revision.relations,
    });
    expect(value.document).not.toHaveProperty('text');
    expect(value).not.toHaveProperty('revisionId');
    expect(JSON.stringify(value)).not.toMatch(/grant-a|account-a|allowModelProcessing|allowExport/);
  });
  it('uses UTF8 byte offsets and preserves combining characters, emoji and edge whitespace without normalization', () => {
    const prefix = '  Анна → Борис\r\n';
    const selected = '«Ёлка» 👩🏽‍🔬';
    const start = encoder.encode(prefix).length;
    const end = start + encoder.encode(selected).length;
    const value = projectBoundExcerpt(binding, revision, { ...span, start, end });
    expect(value.text).toBe(selected);
    expect(value.span).toEqual({ ...span, start, end });
    expect(value.document.authorLabel).toBe('Анна');
    expect(value.document.sourceUrl).toBe('https://example.org/Exact-ID');
  });
  it.each([
    { documentId: 'foreign' },
    { revisionId: 'foreign' },
    { start: -1 },
    { start: 0.5 },
    { end: Number.MAX_SAFE_INTEGER + 1 },
    { start: 2, end: 2 },
    { end: span.end + 1 },
    { start: 3 },
    { end: 3 },
    { unit: 'utf16' },
  ])('rejects mismatched references, invalid bounds or split UTF8 codepoints: %j', (changes) => {
    expect(() =>
      projectBoundExcerpt(binding, revision, { ...span, ...changes } as SourceSpan),
    ).toThrow();
  });
  it('rejects invalid Unicode rather than replacing original source characters', () => {
    const invalid = { ...revision, document: { ...revision.document, text: '\ud800' } };
    expect(() => projectBoundExcerpt(binding, invalid, { ...span, end: 3 })).toThrow();
  });
  it('retains permitted context authors even when a display label matches the selected person', () => {
    const context = {
      ...revision,
      document: {
        ...revision.document,
        authorId: 'author-b',
        authorLabel: 'Анна',
        authorship: 'other' as const,
      },
    };
    const value = projectBoundExcerpt(binding, context, span);
    expect(value.text).toBe(context.document.text);
    expect(value.speakerRelation).toBe('context');
    expect(value.canSupportOwnWords).toBe(false);
    expect(value.document.authorship).toBe('other');
  });
  it.each(['unknown', 'other'] as const)(
    'does not promote %s source authorship for a matching target ID',
    (authorship) => {
      const value = projectBoundExcerpt(
        binding,
        { ...revision, document: { ...revision.document, authorship } },
        span,
      );
      expect(value.canSupportOwnWords).toBe(false);
      expect(value.document.authorship).toBe(authorship);
    },
  );
  it.each(['forward', 'quote'] as const)(
    'preserves %s metadata and conservatively denies own-word support',
    (kind) => {
      const related = {
        ...revision,
        relations: [
          {
            kind,
            original: null,
            originalAuthor: { provider: 'telegram' as const, id: 'author-b', label: 'Борис' },
            span: null,
          },
        ],
      };
      const value = projectBoundExcerpt(binding, related, span);
      expect(value.relations).toEqual(related.relations);
      expect(value.canSupportOwnWords).toBe(false);
      expect(value.text).toBe(revision.document.text);
    },
  );
  it('does not expose an unrelated runtime field attached to span or source metadata', () => {
    const enriched = { ...revision, document: { ...revision.document, token: 'synthetic-secret' } };
    const enrichedSpan = { ...span, grantId: 'synthetic-secret' };
    const value = projectBoundExcerpt(binding, enriched, enrichedSpan);
    expect(JSON.stringify(value)).not.toContain('synthetic-secret');
  });
});

it('preserves a source BOM as text at an excerpt boundary', () => {
  const original = { ...revision, document: { ...revision.document, text: '\ufeffАнна\r\n' } };
  const value = projectBoundExcerpt(binding, original, {
    ...span,
    end: encoder.encode(original.document.text).length,
  });
  expect(value.text).toBe(original.document.text);
});
