export { validateExperiment } from './validation';
export { runExperiment } from './experiment/run-experiment';
export { buildTrialOrder } from './experiment/protocol';
export type { TrialResult, ExperimentPerson, ExperimentResult } from './experiment/result';
export type { RunOptions } from './experiment/run-options';
export type { StageTrace, ProgressEvent } from './execution/types';
export { exportExperiment } from './export/export-experiment';
export { decodePersonaPreset, encodePersonaPreset } from './presets/persona-preset-codec';
export type { StoredPersonaInput, StoredPersonaPreset } from './presets/persona-preset-codec';

export { parsePersonaPreset, serializePersonaPreset } from './presets/persona-preset';
export { decodeLegacyV1Result } from './history/legacy-v1';
export type { LegacyV1Result } from './history/legacy-v1';
