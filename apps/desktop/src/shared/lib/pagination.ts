export function paginateText(text: string, columns: number, rows: number): string[] {
  const width = Math.max(1, Math.floor(columns));
  const height = Math.max(1, Math.floor(rows));
  const lines: string[] = [];
  for (const paragraph of text.replace(/\r\n/gu, '\n').split('\n')) {
    let rest = paragraph;
    while (rest.length > width) {
      const space = rest.lastIndexOf(' ', width);
      const end = space > 0 ? space : width;
      lines.push(rest.slice(0, end));
      rest = rest.slice(end).trimStart();
    }
    lines.push(rest);
  }
  const pages: string[] = [];
  for (let index = 0; index < lines.length; index += height)
    pages.push(lines.slice(index, index + height).join('\n'));
  return pages.length ? pages : [''];
}
