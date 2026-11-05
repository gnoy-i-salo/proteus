import { expect, it } from 'vitest';
import { paginateText } from './pagination';
it('paginates paragraphs and explicit newlines without losing content', () => {
  for (const input of ['abc '.repeat(100), 'line\n'.repeat(50), 'x'.repeat(200)]) {
    const pages = paginateText(input, 25, 4);
    expect(pages.length).toBeGreaterThan(1);
    expect(pages.join('').replace(/\s/gu, '')).toBe(input.replace(/\s/gu, ''));
    for (const page of pages) {
      expect(page.split('\n').length).toBeLessThanOrEqual(4);
      expect(page.split('\n').every((line) => line.length <= 25)).toBe(true);
    }
  }
});
