import { expect, it } from 'vitest';
import { paginateText, paginateInput, locateTextOffset } from './pagination';
it('preserves every response character across page boundaries', () => {
  const original = '  ну...\r\n\r\n**да**  🤷🏽‍♀️\te\u0301\n   конец  ';
  expect(paginateText(original, 5, 2).join('')).toBe(original);
});
it('paginates paragraphs and explicit newlines without losing content', () => {
  for (const input of ['abc '.repeat(100), 'line\n'.repeat(50), 'x'.repeat(200)]) {
    const pages = paginateText(input, 25, 4);
    expect(pages.length).toBeGreaterThan(1);
    expect(pages.join('')).toBe(input);
    for (const page of pages) {
      expect(page.split('\n').length).toBeLessThanOrEqual(4);
      // CSS wraps the original text; pagination must not insert physical newlines.
      const visualRows = page
        .split('\n')
        .reduce((rows, line) => rows + Math.max(1, Math.ceil([...line].length / 25)), 0);
      expect(visualRows).toBeLessThanOrEqual(4);
    }
  }
});

it('paginates editable text without changing spaces, newlines or Unicode', () => {
  const text = 'Привет  мир ✨\n'.repeat(60);
  const pages = paginateInput(text, 12, 3);
  expect(pages.length).toBeGreaterThan(1);
  expect(pages.join('')).toBe(text);
});

it('locates the same original offset after a page capacity change', () => {
  const text = 'first  paragraph\n👩🏽‍🔬 original words\nlast paragraph';
  const offset = text.indexOf('original');
  for (const [columns, rows] of [
    [12, 2],
    [7, 3],
    [24, 2],
  ]) {
    const pages = paginateInput(text, columns!, rows!);
    const location = locateTextOffset(pages, offset);
    expect(pages.slice(0, location.page).join('').length + location.offset).toBe(offset);
    expect(pages.join('')).toBe(text);
  }
  expect(locateTextOffset(['abc', 'def'], 3)).toEqual({ page: 1, offset: 0 });
  expect(locateTextOffset(['abc', 'def'], 6)).toEqual({ page: 1, offset: 3 });
  expect(locateTextOffset([''], 0)).toEqual({ page: 0, offset: 0 });
});
