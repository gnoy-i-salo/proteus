import fixture from './testing/persona-profile-snapshot.json';
import { describe, expect, it } from 'vitest';
import { decodePersonaProfileSnapshot } from './persona-profile-snapshot';

const original = '\uFEFF  Маша é 👩🏽‍🔬\r\n\u0000дословно  ';
function snapshot() {
  return structuredClone(fixture);
}
describe('task-free profile snapshot boundary', () => {
  it('preserves every string and returns a detached canonical field order', () => {
    const input = snapshot();
    const reordered = Object.fromEntries(Object.entries(input).reverse());
    const actual = decodePersonaProfileSnapshot(reordered);
    expect(actual).toEqual(input);
    expect(JSON.stringify(actual)).toBe(JSON.stringify(decodePersonaProfileSnapshot(input)));
    input.rules[0]!.instruction = 'changed';
    input.styleExemplars[0]!.text = 'changed';
    expect(actual.rules[0]!.instruction).toBe(original);
    expect(actual.styleExemplars[0]!.text).toBe(original);
  });
  it('keeps an explicitly insufficient profile without inventing evidence', () => {
    const input = snapshot();
    input.rules = [];
    input.sourceStatus = 'synthetic';
    input.styleExemplars = [];
    input.styleProfile.voiceRules = [];
    input.styleProfile.styleExemplarIds = [];
    input.styleProfile.contextModes.forEach((mode) => {
      mode.status = 'insufficient';
    });
    expect(decodePersonaProfileSnapshot(input)).toEqual(input);
  });
  for (const [name, mutate] of [
    [
      'unknown version',
      (x: ReturnType<typeof snapshot>) => {
        x.schemaVersion = 2;
      },
    ],
    [
      'old style version',
      (x: ReturnType<typeof snapshot>) => {
        x.styleProfile.version = 1;
      },
    ],
    [
      'non Unicode text',
      (x: ReturnType<typeof snapshot>) => {
        x.archetype = '\ud800';
      },
    ],
    [
      'wrong palette',
      (x: ReturnType<typeof snapshot>) => {
        x.palette = 'green';
      },
    ],
    [
      'neutral identity',
      (x: ReturnType<typeof snapshot>) => {
        x.sourceStatus = 'neutral';
      },
    ],
    [
      'unknown rule evidence',
      (x: ReturnType<typeof snapshot>) => {
        x.rules[0]!.evidenceIds = [];
      },
    ],
    [
      'conflicting instruction',
      (x: ReturnType<typeof snapshot>) => {
        x.rules[0]!.status = 'conflicting';
      },
    ],
    [
      'duplicate rules',
      (x: ReturnType<typeof snapshot>) => {
        x.rules.push(x.rules[0]!);
      },
    ],
    [
      'duplicate samples',
      (x: ReturnType<typeof snapshot>) => {
        x.styleExemplars.push(x.styleExemplars[0]!);
      },
    ],
    [
      'missing sample',
      (x: ReturnType<typeof snapshot>) => {
        x.styleExemplars = [];
      },
    ],
    [
      'foreign style reference',
      (x: ReturnType<typeof snapshot>) => {
        x.styleProfile.voiceRules[0]!.exemplarIds = ['foreign'];
      },
    ],
    [
      'duplicate style rules',
      (x: ReturnType<typeof snapshot>) => {
        x.styleProfile.voiceRules.push(x.styleProfile.voiceRules[0]!);
      },
    ],
    [
      'duplicate mode',
      (x: ReturnType<typeof snapshot>) => {
        x.styleProfile.contextModes[1]!.mode = 'dialogue';
      },
    ],
    [
      'invented observed mode',
      (x: ReturnType<typeof snapshot>) => {
        x.styleProfile.contextModes[1]!.status = 'observed';
      },
    ],
    [
      'invented export permission',
      (x: ReturnType<typeof snapshot>) => {
        x.styleExemplars[0]!.exportAllowed = true;
      },
    ],
    [
      'redacted sample',
      (x: ReturnType<typeof snapshot>) => {
        Object.assign(x.styleExemplars[0]!, { redactions: [{ start: 0, end: 1 }] });
      },
    ],
    [
      'observed reasoning',
      (x: ReturnType<typeof snapshot>) => {
        Object.assign(x.styleProfile, { reasoningPatterns: [x.styleProfile.voiceRules[0]] });
        x.styleProfile.voiceRules = [];
      },
    ],
  ] as const) {
    it(`rejects ${name}`, () => {
      const input = snapshot();
      mutate(input);
      expect(() => decodePersonaProfileSnapshot(input)).toThrow();
    });
  }
  for (const field of ['memory', 'id', 'taskSkills', 'skills', 'sourceContext', 'source-status']) {
    it(`rejects ${field} in the task-free body`, () => {
      expect(() => decodePersonaProfileSnapshot({ ...snapshot(), [field]: [] })).toThrow();
    });
  }
  it('rejects unknown nested fields and omitted required fields', () => {
    const input = snapshot();
    expect(() => decodePersonaProfileSnapshot({ ...input, label: undefined })).toThrow();
    Object.assign(input.rules[0]!, { evidence: [] });
    expect(() => decodePersonaProfileSnapshot(input)).toThrow();
  });
  it('enforces the complete UTF8 body budget without truncating strings', () => {
    const input = snapshot();
    input.styleExemplars[0]!.context = 'я'.repeat(262144);
    expect(() => decodePersonaProfileSnapshot(input)).toThrow();
  });
});
