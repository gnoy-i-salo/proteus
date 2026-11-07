import { describe, expect, it } from 'vitest';
import { decodeResponseFields } from './response-fields-codec';
import type { ResponseFieldPair } from './response-fields-codec';

const fields = [
  ['evidence-ids', 'evidenceIds'],
  ['source-status', 'sourceStatus'],
] as const;

describe('fixed-schema flat model-response field codec', () => {
  it('maps the exact declared legacy fields and leaves semantic values unchanged', () => {
    const input = {
      'evidence-ids': ['e1'],
      'source-status': 'model-reported',
      summary: '  Анна → Борис\r\n👩🏽‍🔬 https://example.org/A-B  ',
    };
    expect(decodeResponseFields(input, fields)).toEqual({
      evidenceIds: ['e1'],
      sourceStatus: 'model-reported',
      summary: input.summary,
    });
    expect(input).toHaveProperty('evidence-ids');
    expect(input).not.toHaveProperty('evidenceIds');
  });

  it('accepts canonical fields without changing stable values or copying nested objects', () => {
    const input = { evidenceIds: ['e1'], sourceStatus: 'model-reported' };
    const result = decodeResponseFields(input, fields);
    expect(result).toEqual(input);
    expect(result).not.toBe(input);
    expect(result.evidenceIds).toBe(input.evidenceIds);
  });

  it('never traverses opaque maps or derives names from arbitrary keys', () => {
    const nested = {
      'source-status': 'original',
      sourceStatus: 'also original',
      access_token: 'synthetic',
      'file-name.json': { 'evidence-ids': ['e2'] },
    };
    const input = { 'evidence-ids': ['e1'], 'external-field': nested, metadata: [nested] };
    const result = decodeResponseFields(input, fields);
    expect(result).toEqual({ evidenceIds: ['e1'], 'external-field': nested, metadata: [nested] });
    expect(result['external-field']).toBe(nested);
    expect(result.metadata).toBe(input.metadata);
  });

  it('requires explicit mapping at each schema-owned nested boundary', () => {
    const rule = { 'evidence-ids': ['e1'] };
    const envelope = { 'voice-rules': [rule] };
    const decoded = decodeResponseFields(envelope, [['voice-rules', 'voiceRules']]);
    expect(decoded.voiceRules).toBe(envelope['voice-rules']);
    expect(rule).toEqual({ 'evidence-ids': ['e1'] });
    expect(decodeResponseFields(rule, fields)).toEqual({ evidenceIds: ['e1'] });
  });

  it.each([
    { 'evidence-ids': ['e1'], evidenceIds: ['e1'] },
    { 'evidence-ids': ['e1'], evidenceIds: ['e2'] },
    { 'evidence-ids': ['e1'], sourceStatus: 'known' },
    { evidenceIds: ['e1'], 'source-status': 'known' },
    { 'evidence-ids': undefined, evidenceIds: undefined },
    { 'evidence-ids': null, sourceStatus: null },
  ])('rejects any mixed mapped schemas, including equal and undefined aliases', (input) => {
    expect(() => decodeResponseFields(input, fields)).toThrow(/mixed/i);
  });

  it('preserves absence, explicit undefined, null and invalid semantic values for the owning parser', () => {
    expect(decodeResponseFields({}, fields)).toEqual({});
    const result = decodeResponseFields(
      { 'evidence-ids': undefined, 'source-status': null },
      fields,
    );
    expect(Object.hasOwn(result, 'evidenceIds')).toBe(true);
    expect(result.evidenceIds).toBeUndefined();
    expect(result.sourceStatus).toBeNull();
    expect(decodeResponseFields({ 'evidence-ids': 42 }, fields)).toEqual({ evidenceIds: 42 });
  });

  it('does not mutate frozen input or its nested values', () => {
    const nested = Object.freeze({ 'raw-key': '  exact\r\n' });
    const input = Object.freeze({ 'evidence-ids': Object.freeze(['e1']), nested });
    expect(decodeResponseFields(input, fields)).toEqual({ evidenceIds: ['e1'], nested });
    expect(input).toHaveProperty('evidence-ids');
  });

  it.each([null, undefined, [], 'text', 42, false])('rejects non-record input', (input) => {
    expect(() => decodeResponseFields(input, fields)).toThrow(/object/i);
  });

  it.each<readonly ResponseFieldPair[]>([
    [
      ['old-a', 'newA'],
      ['old-a', 'newB'],
    ],
    [
      ['old-a', 'newA'],
      ['old-b', 'newA'],
    ],
    [
      ['old-a', 'newA'],
      ['newA', 'newB'],
    ],
    [['same', 'same']],
    [['', 'newA']],
    [['old-a', '']],
  ])('rejects an ambiguous schema map independently of payload', (...mapping) => {
    expect(() => decodeResponseFields({}, mapping)).toThrow(/schema/i);
  });

  it('preserves an opaque __proto__ field without assigning the result prototype', () => {
    const input: unknown = JSON.parse('{"__proto__":{"polluted":true},"evidence-ids":["e1"]}');
    const result = decodeResponseFields(input, fields);
    expect(Object.hasOwn(result, '__proto__')).toBe(true);
    expect(result['__proto__']).toEqual({ polluted: true });
    expect(Object.getPrototypeOf(result)).toBe(Object.prototype);
    expect(result.evidenceIds).toEqual(['e1']);
    expect({}).not.toHaveProperty('polluted');
  });

  it('returns a shallow unchanged record for an empty schema map', () => {
    const input = { access_token: 'synthetic', 'public-urls': ['https://example.org'] };
    expect(decodeResponseFields(input, [])).toEqual(input);
  });
});
