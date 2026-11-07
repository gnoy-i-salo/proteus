export interface TextSelection {
  readonly start: number;
  readonly end: number;
  readonly direction: 'forward' | 'backward' | 'none';
}
export interface TextPage {
  readonly start: number;
  readonly end: number;
}
export interface TextLayout {
  readonly width: number;
  readonly rows: number;
  readonly measure: (text: string) => number;
}
const graphemes = new Intl.Segmenter(undefined, { granularity: 'grapheme' });

// Offsets are UTF-16, as in native textarea selections. Pages never rewrite the source.
export function documentPages(text: string, layout: TextLayout): TextPage[] {
  if (!text) return [{ start: 0, end: 0 }];
  const pieces = [...graphemes.segment(text)];
  const pages: TextPage[] = [];
  let first = 0;
  while (first < pieces.length) {
    let row = 1;
    let width = 0;
    let next = first;
    let paragraphEnd = first;
    let paragraphRows = 0;
    let wordEnd = first;
    for (; next < pieces.length; next++) {
      const character = pieces[next]!.segment;
      const measured = character === '\t' ? layout.measure('    ') : layout.measure(character);
      if (character === '\n' || character === '\r\n' || character === '\r') {
        if (row >= Math.max(1, layout.rows)) break;
        row++;
        width = 0;
        paragraphEnd = next + 1;
        paragraphRows = row;
      } else {
        if (width > 0 && width + measured > Math.max(1, layout.width)) {
          row++;
          width = 0;
        }
        if (row > Math.max(1, layout.rows)) break;
        width += measured;
        if (/\s/u.test(character)) wordEnd = next + 1;
      }
    }
    // Prefer a nearby paragraph/word boundary, without turning pages into tiny fragments.
    const minimum = first + Math.floor((next - first) * 0.55);
    if (next < pieces.length) {
      if (paragraphEnd > first && paragraphRows >= Math.max(1, layout.rows) * 0.55)
        next = paragraphEnd;
      else if (wordEnd > minimum) next = wordEnd;
    }
    next = Math.max(first + 1, next);
    pages.push({ start: pieces[first]!.index, end: pieces[next]?.index ?? text.length });
    first = next;
  }
  return pages;
}
export function pageForOffset(pages: readonly TextPage[], offset: number): number {
  return Math.max(
    0,
    pages.findIndex((page, index) => offset < page.end || index === pages.length - 1),
  );
}
export function projectSelection(selection: TextSelection, page: TextPage) {
  return {
    start: Math.max(0, Math.min(page.end, selection.start) - page.start),
    end: Math.max(0, Math.min(page.end, selection.end) - page.start),
    direction: selection.direction,
    continues: selection.start < page.start || selection.end > page.end,
  };
}
export function replaceSelection(text: string, selection: TextSelection, inserted: string) {
  const offset = selection.start + inserted.length;
  return {
    text: text.slice(0, selection.start) + inserted + text.slice(selection.end),
    selection: { start: offset, end: offset, direction: 'none' } satisfies TextSelection,
  };
}
export function findTextMatches(text: string, query: string): TextPage[] {
  if (!query) return [];
  // Literal, case-sensitive search keeps offsets exact even for Unicode case expansions.
  const matches: TextPage[] = [];
  let start = text.indexOf(query);
  while (start !== -1) {
    matches.push({ start, end: start + query.length });
    start = text.indexOf(query, start + Math.max(1, query.length));
  }
  return matches;
}
export function textOutline(text: string): { offset: number; label: string }[] {
  const sections: { offset: number; label: string }[] = [];
  const lines = text.matchAll(/[^\r\n]+/gu);
  for (const line of lines) {
    if (line.index === 0 || text.slice(Math.max(0, line.index - 2), line.index).includes('\n')) {
      const label = line[0].trim();
      if (label) sections.push({ offset: line.index, label: label.slice(0, 80) });
    }
  }
  return sections;
}

// Textareas normalize CRLF. Translate DOM offsets instead of normalizing stored source text.
export function sourceOffset(text: string, domOffset: number): number {
  let dom = 0;
  let source = 0;
  while (source < text.length && dom < domOffset) {
    source += text[source] === '\r' && text[source + 1] === '\n' ? 2 : 1;
    dom++;
  }
  return source;
}
export function domOffset(text: string, offset: number): number {
  return text.slice(0, offset).replace(/\r\n?/g, '\n').length;
}
export function applyPageEdit(original: string, changed: string) {
  const before = original.replace(/\r\n?/g, '\n');
  let start = 0;
  while (start < before.length && start < changed.length && before[start] === changed[start])
    start++;
  let end = before.length;
  let changedEnd = changed.length;
  while (end > start && changedEnd > start && before[end - 1] === changed[changedEnd - 1]) {
    end--;
    changedEnd--;
  }
  return (
    original.slice(0, sourceOffset(original, start)) +
    changed.slice(start, changedEnd) +
    original.slice(sourceOffset(original, end))
  );
}
export function moveTextOffset(text: string, offset: number, direction: -1 | 1): number {
  const boundaries = [...graphemes.segment(text)].map((piece) => piece.index);
  boundaries.push(text.length);
  return direction > 0
    ? (boundaries.find((value) => value > offset) ?? text.length)
    : (boundaries.filter((value) => value < offset).at(-1) ?? 0);
}
export function selectedCharacterCount(text: string): number {
  return [...graphemes.segment(text)].length;
}
