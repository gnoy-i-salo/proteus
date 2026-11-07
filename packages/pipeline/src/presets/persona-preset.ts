import type { ExperimentInput } from '@proteus-int/contracts';
import { decodePersonaPreset, encodePersonaPreset } from './persona-preset-codec';

export function parsePersonaPreset(data: string): ExperimentInput {
  return decodePersonaPreset(data);
}

export function serializePersonaPreset(input: ExperimentInput): string {
  return encodePersonaPreset(input);
}
