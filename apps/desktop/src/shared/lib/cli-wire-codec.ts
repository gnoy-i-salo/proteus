import type {
  IntelligenceEffort,
  IntelligenceOperation,
  ModelContextReference,
  ProviderId,
  ReasoningBlock,
} from '@proteus-int/contracts';

// The live native boundary is canonical; legacy decoders remain explicit compatibility readers.
export interface CliWireTrace {
  readonly provider: ProviderId;
  /** Compatibility duration: wall time, never measured active reasoning. */
  readonly reasoningMs: number;
  readonly wallTimeMs?: number;
  readonly activeReasoningMs?: null;
  readonly requestedModel?: string;
  readonly resolvedModel?: string | null;
  readonly requestedEffort?: IntelligenceEffort;
  readonly appliedEffort?: IntelligenceEffort | null;
  readonly cliVersion?: string | null;
  readonly terminalMs: number;
  readonly summary: string;
  readonly reasoning?: readonly ReasoningBlock[];
  readonly reasoningTruncated?: boolean;
}

export interface CliWireResult {
  readonly answer: string;
  readonly trace: CliWireTrace;
}

export interface CliWireEvent {
  readonly id: string;
  readonly kind: 'started' | 'reasoning' | 'answer' | 'search' | 'finished';
  readonly elapsedMs: number;
  readonly reasoning?: ReasoningBlock;
}

// This exact old DTO exists only at the native boundary, never in domain interfaces.
export interface LegacyCliRequest {
  readonly id: string;
  readonly provider: ProviderId;
  readonly model: string;
  readonly task: string;
  readonly 'system-prompt': string;
  readonly effort: IntelligenceEffort;
  readonly operation?: IntelligenceOperation;
  readonly 'source-context'?: boolean;
}

const encoder = new TextEncoder();
const publicOutputBytes = 4 * 1024 * 1024;
const reasoningBytes = 48 * 1024;

function fields(value: unknown, allowed: readonly string[]): Record<string, unknown> {
  if (!value || typeof value !== 'object' || Array.isArray(value))
    throw new Error('Неверный формат данных CLI.');
  if (Object.keys(value).some((key) => !allowed.includes(key)))
    throw new Error('Неизвестные или смешанные поля CLI.');
  return value as Record<string, unknown>;
}

function text(value: unknown, maxBytes = publicOutputBytes): string {
  if (typeof value !== 'string' || encoder.encode(value).length > maxBytes)
    throw new Error('Неверное или слишком длинное текстовое поле CLI.');
  return value;
}

function nullableText(value: unknown): string | null {
  return value === null ? null : text(value);
}

function choice<T extends string>(value: unknown, choices: readonly T[]): T {
  const found = choices.find((item) => item === value);
  if (!found) throw new Error('Неизвестное значение поля CLI.');
  return found;
}

function effort(value: unknown): IntelligenceEffort {
  return choice(value, ['low', 'medium', 'high'] as const);
}

function duration(value: unknown): number {
  if (typeof value !== 'number' || !Number.isSafeInteger(value) || value < 0)
    throw new Error('Неверная длительность CLI.');
  return value;
}

function boolean(value: unknown): boolean {
  if (typeof value !== 'boolean') throw new Error('Неверный флаг CLI.');
  return value;
}

function unknownDuration(value: unknown): null {
  if (value !== null) throw new Error('CLI не измеряет активное время reasoning.');
  return null;
}

function reasoningBlock(value: unknown): ReasoningBlock {
  const block = fields(value, ['id', 'kind', 'text']);
  return {
    id: text(block['id']),
    kind: choice(block['kind'], ['summary', 'thinking'] as const),
    text: text(block['text'], reasoningBytes),
  };
}

function reasoningBlocks(value: unknown): readonly ReasoningBlock[] {
  if (!Array.isArray(value) || value.length > 64)
    throw new Error('Неверное число блоков reasoning CLI.');
  const blocks = value.map(reasoningBlock);
  if (
    new Set(blocks.map((block) => block.id)).size !== blocks.length ||
    blocks.reduce((total, block) => total + encoder.encode(block.text).length, 0) > reasoningBytes
  )
    throw new Error('Повторяющиеся или слишком длинные блоки reasoning CLI.');
  return blocks;
}

function trace(value: unknown, legacy: boolean): CliWireTrace {
  const keys = {
    reasoningMs: legacy ? 'reasoning-ms' : 'reasoningMs',
    wallTimeMs: legacy ? 'wall-time-ms' : 'wallTimeMs',
    activeReasoningMs: legacy ? 'active-reasoning-ms' : 'activeReasoningMs',
    requestedModel: legacy ? 'requested-model' : 'requestedModel',
    resolvedModel: legacy ? 'resolved-model' : 'resolvedModel',
    requestedEffort: legacy ? 'requested-effort' : 'requestedEffort',
    appliedEffort: legacy ? 'applied-effort' : 'appliedEffort',
    cliVersion: legacy ? 'cli-version' : 'cliVersion',
    terminalMs: legacy ? 'terminal-ms' : 'terminalMs',
    reasoningTruncated: legacy ? 'reasoning-truncated' : 'reasoningTruncated',
  };
  const source = fields(value, [
    'provider',
    keys.reasoningMs,
    keys.wallTimeMs,
    keys.activeReasoningMs,
    keys.requestedModel,
    keys.resolvedModel,
    keys.requestedEffort,
    keys.appliedEffort,
    keys.cliVersion,
    keys.terminalMs,
    'summary',
    'reasoning',
    keys.reasoningTruncated,
  ]);
  return {
    provider: choice(source['provider'], ['codex', 'kimi'] as const),
    reasoningMs: duration(source[keys.reasoningMs]),
    terminalMs: duration(source[keys.terminalMs]),
    summary: text(source['summary']),
    ...(keys.wallTimeMs in source ? { wallTimeMs: duration(source[keys.wallTimeMs]) } : {}),
    ...(keys.activeReasoningMs in source
      ? { activeReasoningMs: unknownDuration(source[keys.activeReasoningMs]) }
      : {}),
    ...(keys.requestedModel in source ? { requestedModel: text(source[keys.requestedModel]) } : {}),
    ...(keys.resolvedModel in source
      ? { resolvedModel: nullableText(source[keys.resolvedModel]) }
      : {}),
    ...(keys.requestedEffort in source
      ? { requestedEffort: effort(source[keys.requestedEffort]) }
      : {}),
    ...(keys.appliedEffort in source
      ? {
          appliedEffort:
            source[keys.appliedEffort] === null ? null : effort(source[keys.appliedEffort]),
        }
      : {}),
    ...(keys.cliVersion in source ? { cliVersion: nullableText(source[keys.cliVersion]) } : {}),
    ...('reasoning' in source ? { reasoning: reasoningBlocks(source['reasoning']) } : {}),
    ...(keys.reasoningTruncated in source
      ? { reasoningTruncated: boolean(source[keys.reasoningTruncated]) }
      : {}),
  };
}

export function decodeLegacyCliTrace(value: unknown): CliWireTrace {
  return trace(value, true);
}
export function decodeCliTrace(value: unknown): CliWireTrace {
  return trace(value, false);
}
export function decodeCliResult(value: unknown): CliWireResult {
  const source = fields(value, ['answer', 'trace']);
  return { answer: text(source['answer']), trace: decodeCliTrace(source['trace']) };
}

export function decodeLegacyCliResult(value: unknown): CliWireResult {
  const source = fields(value, ['answer', 'trace']);
  return { answer: text(source['answer']), trace: decodeLegacyCliTrace(source['trace']) };
}

function event(value: unknown, legacy: boolean): CliWireEvent {
  const elapsedKey = legacy ? 'elapsed-ms' : 'elapsedMs';
  const source = fields(value, ['id', 'kind', elapsedKey, 'reasoning']);
  const kind = choice(source['kind'], [
    'started',
    'reasoning',
    'answer',
    'search',
    'finished',
  ] as const);
  const hasReasoning = 'reasoning' in source;
  if ((kind === 'reasoning') !== hasReasoning)
    throw new Error('Блок reasoning не соответствует событию CLI.');
  const id = text(source['id'], 80);
  if (!/^[a-zA-Z0-9-]+$/u.test(id)) throw new Error('Неверный ID события CLI.');
  return {
    id,
    kind,
    elapsedMs: duration(source[elapsedKey]),
    ...('reasoning' in source ? { reasoning: reasoningBlock(source['reasoning']) } : {}),
  };
}

export function decodeLegacyCliEvent(value: unknown): CliWireEvent {
  return event(value, true);
}
export function decodeCliEvent(value: unknown): CliWireEvent {
  return event(value, false);
}
export interface CliWireRequest {
  readonly id: string;
  readonly provider: ProviderId;
  readonly model: string;
  readonly task: string;
  readonly systemPrompt: string;
  readonly effort: IntelligenceEffort;
  readonly operation?: IntelligenceOperation;
  readonly sourceContext?: boolean;
  readonly memoryContext?: ModelContextReference;
}
function memoryContext(value: unknown): ModelContextReference {
  const source = fields(value, ['sessionId', 'leaseId']);
  const sessionId = text(source['sessionId'], 128);
  const leaseId = text(source['leaseId'], 128);
  if (![sessionId, leaseId].every((id) => /^[a-zA-Z0-9:_-]+$/u.test(id)))
    throw new Error('Неверная ссылка на контекст памяти.');
  return { sessionId, leaseId };
}
export function encodeCliRequest(value: unknown): CliWireRequest {
  const source = fields(value, [
    'id',
    'provider',
    'model',
    'task',
    'systemPrompt',
    'effort',
    'operation',
    'sourceContext',
    'memoryContext',
    'formatAttempt',
  ]);
  const id = text(source['id'], 80);
  const model = 'model' in source ? text(source['model'], 120) : '';
  if (
    !/^[a-zA-Z0-9-]+$/u.test(id) ||
    ['\r', '\n', '\0'].some((character) => model.includes(character))
  )
    throw new Error('Неверный ID запроса или псевдоним модели CLI.');
  // Repair attempt is domain bookkeeping and is not a native Request field.
  if ('formatAttempt' in source && source['formatAttempt'] !== 1 && source['formatAttempt'] !== 2)
    throw new Error('Неверная попытка исправления формата.');
  if (
    'memoryContext' in source &&
    (source['sourceContext'] !== true ||
      !['corpus-analysis', 'persona-reply', 'generation'].includes(String(source['operation'])))
  )
    throw new Error('Контекст памяти требует операции с источниками.');
  return {
    id,
    provider: choice(source['provider'], ['codex', 'kimi'] as const),
    model,
    task: text(source['task'], 200_000),
    systemPrompt: text(source['systemPrompt'], 40_000),
    effort: effort(source['effort']),
    ...('operation' in source
      ? {
          operation: choice(source['operation'], [
            'generation',
            'public-research',
            'source-plan',
            'corpus-analysis',
            'persona-reply',
          ] as const),
        }
      : {}),
    ...('sourceContext' in source ? { sourceContext: boolean(source['sourceContext']) } : {}),
    ...('memoryContext' in source ? { memoryContext: memoryContext(source['memoryContext']) } : {}),
  };
}

export function encodeLegacyCliRequest(value: unknown): LegacyCliRequest {
  const source = encodeCliRequest(value);
  if (source.memoryContext !== undefined)
    throw new Error('Старый формат CLI не поддерживает контекст памяти.');
  return {
    id: source.id,
    provider: source.provider,
    model: source.model,
    task: source.task,
    'system-prompt': source.systemPrompt,
    effort: source.effort,
    ...('operation' in source ? { operation: source.operation } : {}),
    ...('sourceContext' in source ? { 'source-context': source.sourceContext } : {}),
  };
}

export interface CliWireStatus {
  readonly provider: ProviderId;
  readonly available: boolean;
  readonly path: string | null;
}
export function decodeCliStatuses(value: unknown): CliWireStatus[] {
  if (!Array.isArray(value)) throw new Error('Неверный список доступности CLI.');
  const statuses = value.map((item: unknown): CliWireStatus => {
    const source = fields(item, ['provider', 'available', 'path']);
    const available = boolean(source['available']);
    const path = source['path'];
    if ((path !== null && typeof path !== 'string') || available !== (path !== null))
      throw new Error('Неверные данные доступности CLI.');
    return { provider: choice(source['provider'], ['codex', 'kimi'] as const), available, path };
  });
  if (new Set(statuses.map((status) => status.provider)).size !== statuses.length)
    throw new Error('Повторяющийся провайдер CLI.');
  return statuses;
}
export function decodeSavedExperimentPath(value: unknown): string {
  if (typeof value !== 'string') throw new Error('Неверный путь сохранённого эксперимента.');
  return value;
}
