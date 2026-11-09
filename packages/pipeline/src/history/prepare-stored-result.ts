import type { StoredResult } from './read-stored-result';
import { exportExperiment } from '../export/export-experiment';
import { decodeV4Result } from './result-v4';
export interface PreparedStoredResult {
  readonly stored: StoredResult;
  readonly view: StoredResult;
}
export function prepareStoredResult(stored: StoredResult): PreparedStoredResult {
  // The original remains available for provenance, never displayed as a newly
  // authorized source. The view uses the same policy as explicit result export.
  const view: StoredResult =
    stored.sourceVersion === 4
      ? { sourceVersion: 4, result: decodeV4Result(JSON.parse(exportExperiment(stored.result))) }
      : stored;
  return { stored, view };
}
