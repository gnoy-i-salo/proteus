import { describe, expect, it } from 'vitest';
import {
  applyPageEdit,
  sourceOffset,
  domOffset,
  documentPages,
  projectSelection,
  replaceSelection,
  findTextMatches,
} from './text-document';

describe('text pages preserve meaningful boundaries', () => {
  it('keeps a paragraph together when it fits the next page without changing source', () => {
    const text = 'Первый абзац.\n\nВторой целый абзац.';
    const pages = documentPages(text, { width: 16, rows: 3, measure: (value) => value.length }).map(
      (page) => text.slice(page.start, page.end),
    );
    expect(pages.join('')).toBe(text);
    expect(pages[0]).toBe('Первый абзац.\n\n');
    expect(pages[1]).toBe('Второй целый абзац.');
  });
});

// The projected DOM range may be shorter; the source range is authoritative.
it('retains a backward selection across several pages for copy and replacement', () => {
  const text = '  Привет 👩🏽‍🔬\nточный suffix  ';
  const selection = { start: 2, end: text.length - 2, direction: 'backward' } as const;
  const projection = projectSelection(selection, { start: 0, end: 8 });
  expect(projection).toEqual({ start: 2, end: 8, direction: 'backward', continues: true });
  expect(text.slice(selection.start, selection.end)).toBe('Привет 👩🏽‍🔬\nточный suffix');
  expect(replaceSelection(text, selection, '✨\n')).toEqual({
    text: '  ✨\n  ',
    selection: { start: 4, end: 4, direction: 'none' },
  });
});
it('never divides emoji graphemes, normalizes whitespace or creates an empty trailing page', () => {
  const text = '👩🏽‍🔬  A\r\nB\t\n';
  const pages = documentPages(text, { width: 2, rows: 2, measure: () => 1 });
  const chunks = pages.map((page) => text.slice(page.start, page.end));
  expect(chunks.join('')).toBe(text);
  expect(chunks[0]).toContain('👩🏽‍🔬');
  expect(chunks.every((value) => value.length > 0)).toBe(true);
});
it('search is literal and keeps UTF16 offsets for exact Unicode matches', () => {
  expect(findTextMatches(' A👩🏽‍🔬 A👩🏽‍🔬 ', 'A👩🏽‍🔬')).toEqual([
    { start: 1, end: 9 },
    { start: 10, end: 18 },
  ]);
  expect(findTextMatches('a.*b', '.*')).toEqual([{ start: 1, end: 3 }]);
  expect(findTextMatches('abc', '')).toEqual([]);
});

it('edits only the changed span and preserves untouched CRLF and standalone CR', () => {
  const source = 'A\r\n  👩🏽‍🔬 B\rC\n';
  const changed = 'A\n  👩🏽‍🔬 X\nC\n';
  expect(applyPageEdit(source, changed)).toBe('A\r\n  👩🏽‍🔬 X\rC\n');
  expect(sourceOffset(source, 2)).toBe(3);
  expect(domOffset(source, 3)).toBe(2);
});
