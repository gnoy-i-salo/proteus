export function paginateText(text: string, columns: number, rows: number): string[] {
  return paginateInput(text, columns, rows);
}

// Keep original characters: visual wrapping must never change exported preset data.
export function paginateInput(text: string, columns: number, rows: number): string[] {
  const width = Math.max(1, Math.floor(columns));
  const height = Math.max(1, Math.floor(rows));
  const pages: string[] = [];
  let page = '';
  let column = 0;
  let row = 1;
  for (const character of text) {
    if (character !== '\n' && column >= width) {
      row++;
      column = 0;
    }
    if (row > height || (character === '\n' && row === height)) {
      pages.push(page);
      page = '';
      row = 1;
      column = 0;
    }
    page += character;
    if (character === '\n') {
      row++;
      column = 0;
    } else column++;
  }
  if (page || !pages.length) pages.push(page);
  return pages;
}
