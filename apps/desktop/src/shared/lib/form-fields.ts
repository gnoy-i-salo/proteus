export function formText(fields: FormData, name: string): string {
  const value = fields.get(name);
  return typeof value === 'string' ? value : '';
}
