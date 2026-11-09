import { decodeSeriesResult } from './result-series';
import type { StoredSeriesResult } from './result-series';
export type StoredV4Result = StoredSeriesResult<4>;
export type { StoredExportMetadata } from './result-series';
export function decodeV4Result(value: unknown): StoredV4Result {
  return decodeSeriesResult(value, 4);
}
