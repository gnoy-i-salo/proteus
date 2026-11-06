import type { PersonInput, SourceDocument, SourceGrant } from '@proteus-int/contracts';

export interface SourceReplacement {
  readonly start: number;
  readonly end: number;
}

export function sanitizeSourceText(
  text: string,
  input: PersonInput,
  grant: SourceGrant,
  documents: readonly SourceDocument[],
): { text: string; replacements: readonly SourceReplacement[] } {
  const identifiers = [
    input.label,
    grant['account-id'],
    grant['resource-id'],
    grant['author-id'],
    ...documents.map((doc) => doc['author-id']),
    ...documents.map((doc) => doc['author-label']),
    ...input['public-urls'],
  ].filter((value): value is string => Boolean(value));
  const escaped = [...new Set(identifiers)]
    .sort((a, b) => b.length - a.length)
    .map((value) => value.replace(/[.*+?^${}()|[\]\\]/gu, '\\$&'));
  const pattern = new RegExp(
    `https?:\\/\\/[^\\s<>]+|(?<![\\w.+-])[\\w.+-]+@[\\w.-]+\\.[a-z]{2,}|@[\\p{L}\\p{N}_.-]+${escaped.length ? `|(?<![\\p{L}\\p{N}_])(?:${escaped.join('|')})(?![\\p{L}\\p{N}_])` : ''}`,
    'giu',
  );
  const replacements: SourceReplacement[] = [];
  let offset = 0;
  const clean = text.replace(pattern, (matched: string, index: number) => {
    const start = index + offset;
    const replacement = '[person]';
    replacements.push({ start, end: start + replacement.length });
    offset += replacement.length - matched.length;
    return replacement;
  });
  return { text: clean, replacements };
}

export function sourceModelDocument(
  document: SourceDocument,
  input: PersonInput,
  grant: SourceGrant,
  documents: readonly SourceDocument[],
) {
  return {
    id: document.id,
    authorship: document.authorship,
    kind: document.kind,
    mode: document.mode,
    text: sanitizeSourceText(document.text, input, grant, documents).text,
    'reply-to': document['reply-to'],
    'is-forwarded': document['is-forwarded'],
    truncated: document.truncated,
  };
}
