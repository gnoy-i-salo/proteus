import { decodeLegacyV1Result } from './legacy-v1';
import type { LegacyV1Result } from './legacy-v1';
import { decodeV4Result } from './result-v4';
import type { StoredV4Result } from './result-v4';
import { parseHistoryEnvelope } from './parse-history-envelope';
export type StoredResult =
  | { readonly sourceVersion: 1; readonly result: LegacyV1Result }
  | { readonly sourceVersion: 4; readonly result: StoredV4Result };
export function readStoredResult(text: string): StoredResult {
  const envelope = parseHistoryEnvelope(text);
  if (envelope.formatVersion === 1)
    return { sourceVersion: 1, result: decodeLegacyV1Result(envelope.value) };
  if (envelope.formatVersion === 4)
    return { sourceVersion: 4, result: decodeV4Result(envelope.value) };
  throw new Error('Эта версия истории ещё не поддерживается.');
}
