import { decodeSeriesResult } from './result-series';
import type { StoredSeriesResult } from './result-series';
export type StoredV5Result = StoredSeriesResult<5>;
export type { StoredExportMetadata } from './result-series';
export function decodeV5Result(value: unknown): StoredV5Result {
  return decodeSeriesResult(value, 5);
}
