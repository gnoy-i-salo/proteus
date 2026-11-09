import { invoke } from '@tauri-apps/api/core';
import { isDesktop } from './window-controls';
import { exactMemoryInput } from './native-memory-session';

export interface SavedExperimentSummary {
  readonly id: string;
  readonly ok: boolean;
  readonly error: string | null;
}
export interface SavedExperimentPage {
  readonly schemaVersion: 1;
  readonly entries: readonly SavedExperimentSummary[];
  readonly matched: number;
  readonly nextOffset: number | null;
  readonly scanTruncated: boolean;
}
export interface SavedExperimentText {
  readonly schemaVersion: 1;
  readonly id: string;
  readonly text: string;
}
const errors = [
  'unreadableRun',
  'symlinkedRun',
  'missingResult',
  'unreadableResult',
  'symlinkedResult',
  'notAFile',
  'oversizeResult',
];
function fields(value: unknown, keys: readonly string[]): Record<string, unknown> {
  if (
    !value ||
    typeof value !== 'object' ||
    Array.isArray(value) ||
    Object.keys(value).length !== keys.length ||
    Object.keys(value).some((key) => !keys.includes(key))
  )
    throw new Error('Неверные поля истории запусков.');
  return value as Record<string, unknown>;
}
function runId(value: unknown): string {
  if (typeof value !== 'string' || !/^[a-zA-Z0-9-]{1,80}$/u.test(value))
    throw new Error('Неверный идентификатор сохранённого запуска.');
  return value;
}
function count(value: unknown, max: number): number {
  if (typeof value !== 'number' || !Number.isSafeInteger(value) || value < 0 || value > max)
    throw new Error('Неверные границы истории запусков.');
  return value;
}
function available(signal?: AbortSignal): void {
  signal?.throwIfAborted();
  if (!isDesktop()) throw new Error('История запусков доступна в настольном PROTEUS.');
}
function summary(value: unknown): SavedExperimentSummary {
  const input = fields(value, ['id', 'ok', 'error']);
  if (
    typeof input.ok !== 'boolean' ||
    (input.ok
      ? input.error !== null
      : typeof input.error !== 'string' || !errors.includes(input.error))
  )
    throw new Error('Неверный статус сохранённого запуска.');
  return { id: runId(input.id), ok: input.ok, error: input.error as string | null };
}
export async function listSavedExperiments(
  offset = 0,
  limit = 50,
  signal?: AbortSignal,
): Promise<SavedExperimentPage> {
  available(signal);
  count(offset, 1024);
  count(limit, 100);
  if (!limit) throw new Error('Страница истории не может быть пустой.');
  const value = await invoke<unknown>('list_saved_experiments', { offset, limit });
  signal?.throwIfAborted();
  const input = fields(value, [
    'schemaVersion',
    'entries',
    'matched',
    'nextOffset',
    'scanTruncated',
  ]);
  if (
    input.schemaVersion !== 1 ||
    !Array.isArray(input.entries) ||
    input.entries.length > limit ||
    typeof input.scanTruncated !== 'boolean'
  )
    throw new Error('Неверная страница истории запусков.');
  const entries = input.entries.map(summary);
  const matched = count(input.matched, 1024);
  const nextOffset = input.nextOffset === null ? null : count(input.nextOffset, 1024);
  const expectedCount = Math.min(limit, Math.max(0, matched - offset));
  const consumed = offset + entries.length;
  if (
    entries.length !== expectedCount ||
    nextOffset !== (consumed < matched ? consumed : null) ||
    new Set(entries.map((entry) => entry.id)).size !== entries.length ||
    entries.some((entry, index) => index > 0 && entries[index - 1]!.id >= entry.id)
  )
    throw new Error('Непоследовательная страница истории запусков.');
  return { schemaVersion: 1, entries, matched, nextOffset, scanTruncated: input.scanTruncated };
}
export async function readSavedExperiment(
  id: string,
  signal?: AbortSignal,
): Promise<SavedExperimentText> {
  available(signal);
  runId(id);
  const value = await invoke<unknown>('read_saved_experiment', { id });
  signal?.throwIfAborted();
  const input = fields(value, ['schemaVersion', 'id', 'text']);
  if (input.schemaVersion !== 1 || input.id !== id || typeof input.text !== 'string')
    throw new Error('Неверный сохранённый результат.');
  exactMemoryInput(input.text, 8 * 1024 * 1024);
  const content: unknown = JSON.parse(input.text);
  if (!content || typeof content !== 'object' || Array.isArray(content))
    throw new Error('Файл результата должен содержать объект JSON.');
  // This boundary preserves the original envelope. The version-aware result
  // reader decides whether its historical schema can be displayed.
  return { schemaVersion: 1, id, text: input.text };
}
