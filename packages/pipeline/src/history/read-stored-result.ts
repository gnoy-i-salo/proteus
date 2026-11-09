import { decodeLegacySeriesResult } from './legacy-series';
import type { LegacySeriesResult } from './legacy-series-types';
import { decodeLegacyV1Result } from './legacy-v1';
import type { LegacyV1Result } from './legacy-v1';
import { decodeV4Result } from './result-v4';
import type { StoredV4Result } from './result-v4';
import { parseHistoryEnvelope } from './parse-history-envelope';
export type StoredResult =
  | { readonly sourceVersion: 1; readonly result: LegacyV1Result }
  | { readonly sourceVersion: 4; readonly result: StoredV4Result }
  | { readonly sourceVersion: 2 | 3; readonly result: LegacySeriesResult };
export function readStoredResult(text: string): StoredResult {
  const envelope = parseHistoryEnvelope(text);
  if (envelope.formatVersion === 1)
    return { sourceVersion: 1, result: decodeLegacyV1Result(envelope.value) };
  if (envelope.formatVersion === 4)
    return { sourceVersion: 4, result: decodeV4Result(envelope.value) };
  return {
    sourceVersion: envelope.formatVersion,
    result: decodeLegacySeriesResult(envelope.value),
  };
}
