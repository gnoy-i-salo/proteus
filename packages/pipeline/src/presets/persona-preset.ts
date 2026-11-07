import type { ExperimentInput } from '@proteus-int/contracts';
import { decodePersonaPreset, encodePersonaPreset } from './persona-preset-codec';

export function parsePersonaPreset(data: string): ExperimentInput {
  return decodePersonaPreset(data);
}

/** Native file validation still accepts v1 until its own serialized boundary migrates. */
export function serializePersonaPreset(input: ExperimentInput): string {
  const clean = decodePersonaPreset(encodePersonaPreset(input));
  return JSON.stringify(
    {
      format: 'proteus-personas',
      version: 1,
      task: clean.task,
      people: clean.people.map((person) => ({
        label: person.label,
        context: person.context,
        'public-urls': person.publicUrls,
      })),
    },
    null,
    2,
  );
}
