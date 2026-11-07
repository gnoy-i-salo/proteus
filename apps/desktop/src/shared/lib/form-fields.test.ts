import { expect, it } from 'vitest';
import { formText } from './form-fields';

it('preserves submitted text exactly', () => {
  const fields = new FormData();
  fields.set('source-url', '  Анна\r\n👩🏽‍🔬  ');
  expect(formText(fields, 'source-url')).toBe('  Анна\r\n👩🏽‍🔬  ');
});
it('returns the existing empty-field default for a missing control', () => {
  expect(formText(new FormData(), 'missing')).toBe('');
});
it('does not stringify uploaded files into text controls', () => {
  const fields = new FormData();
  fields.set('client-id', new File(['not a client ID'], 'unexpected.txt'));
  expect(formText(fields, 'client-id')).toBe('');
});
