export interface LocalModelState {
  readonly schemaVersion: 1;
  readonly modelId: string;
  readonly modelRevision: string;
  readonly packageBytes: number;
  readonly runtimeLoaded: boolean;
  readonly operationActive: boolean;
}
export interface LocalModelProgress {
  readonly schemaVersion: 1;
  readonly operationId: string;
  readonly artifactsVerified: number;
  readonly bytesVerified: number;
}
export type LocalModelPackageState = 'missing' | 'ready';
function fields(value: unknown, keys: readonly string[]): Record<string, unknown> {
  if (!value || typeof value !== 'object' || Array.isArray(value))
    throw new Error('Неверное состояние локальной модели.');
  const actual = Object.keys(value);
  if (actual.length !== keys.length || actual.some((key) => !keys.includes(key)))
    throw new Error('Неизвестные или отсутствующие поля локальной модели.');
  return value as Record<string, unknown>;
}
function counter(value: unknown, minimum: number, maximum: number): number {
  if (
    typeof value !== 'number' ||
    !Number.isSafeInteger(value) ||
    value < minimum ||
    value > maximum
  )
    throw new Error('Неверный счётчик локальной модели.');
  return value;
}
export function decodeLocalModelState(value: unknown): LocalModelState {
  const state = fields(value, [
    'schemaVersion',
    'modelId',
    'modelRevision',
    'packageBytes',
    'runtimeLoaded',
    'operationActive',
  ]);
  if (
    state.schemaVersion !== 1 ||
    state.modelId !== 'intfloat/multilingual-e5-small' ||
    typeof state.modelRevision !== 'string' ||
    !/^[a-f0-9]{40}$/u.test(state.modelRevision) ||
    typeof state.runtimeLoaded !== 'boolean' ||
    typeof state.operationActive !== 'boolean'
  )
    throw new Error('Неподдерживаемое состояние локальной модели.');
  return {
    schemaVersion: 1,
    modelId: state.modelId,
    modelRevision: state.modelRevision,
    packageBytes: counter(state.packageBytes, 1, 1024 ** 3),
    runtimeLoaded: state.runtimeLoaded,
    operationActive: state.operationActive,
  };
}
export function decodeLocalModelProgress(value: unknown, operationId: string): LocalModelProgress {
  const progress = fields(value, [
    'schemaVersion',
    'operationId',
    'artifactsVerified',
    'bytesVerified',
  ]);
  if (
    progress.schemaVersion !== 1 ||
    progress.operationId !== operationId ||
    !operationId.length ||
    operationId.length > 128 ||
    operationId.trim() !== operationId ||
    /\p{Cc}/u.test(operationId)
  )
    throw new Error('Прогресс принадлежит другой операции.');
  const bytes = new TextEncoder().encode(operationId);
  if (
    bytes.length > 128 ||
    new TextDecoder('utf-8', { fatal: true, ignoreBOM: true }).decode(bytes) !== operationId
  )
    throw new Error('Неверный идентификатор операции модели.');
  const artifactsVerified = counter(progress.artifactsVerified, 0, 3);
  const bytesVerified = counter(progress.bytesVerified, 0, 1024 ** 3);
  if ((artifactsVerified === 0) !== (bytesVerified === 0))
    throw new Error('Счётчики проверки файлов не согласованы.');
  return { schemaVersion: 1, operationId, artifactsVerified, bytesVerified };
}
export function decodeLocalModelPackageState(value: unknown): LocalModelPackageState {
  if (value !== 'missing' && value !== 'ready')
    throw new Error('Неверное состояние файлов модели.');
  return value;
}
