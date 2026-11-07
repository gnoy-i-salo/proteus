import { invoke } from '@tauri-apps/api/core';
import { isDesktop } from './window-controls';

export interface SelectedIngestionInput {
  readonly schemaVersion: 1;
  readonly sourceHandle: string;
  readonly label: string;
  readonly inputBytes: number;
}

export async function pickIngestionSource(): Promise<SelectedIngestionInput | null> {
  requireDesktop();
  const value = await invoke<unknown>('pick_ingestion_source');
  if (value === null) return null;
  if (typeof value !== 'object' || Array.isArray(value)) throw invalidSelection();
  const record = value as Record<string, unknown>;
  if (
    Object.keys(record).sort().join(',') !== 'inputBytes,label,schemaVersion,sourceHandle' ||
    record.schemaVersion !== 1 ||
    typeof record.label !== 'string' ||
    !record.label.length ||
    typeof record.inputBytes !== 'number' ||
    !Number.isSafeInteger(record.inputBytes) ||
    record.inputBytes < 0
  )
    throw invalidSelection();
  return {
    schemaVersion: 1,
    sourceHandle: validateHandle(record.sourceHandle),
    label: record.label,
    inputBytes: record.inputBytes,
  };
}

export async function releaseIngestionSource(sourceHandle: string): Promise<void> {
  requireDesktop();
  await invoke<void>('release_ingestion_source', { sourceHandle: validateHandle(sourceHandle) });
}
function requireDesktop(): void {
  if (!isDesktop()) throw new Error('Выбор файлов доступен в настольном приложении PROTEUS.');
}
function invalidSelection(): Error {
  return new Error('Неверный ответ выбора источника.');
}
function validateHandle(value: unknown): string {
  if (typeof value !== 'string' || !/^[\w-]{32}$/.test(value)) throw invalidSelection();
  return value;
}
