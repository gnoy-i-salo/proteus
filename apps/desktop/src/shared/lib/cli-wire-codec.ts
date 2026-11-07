import type {
  IntelligenceEffort,
  IntelligenceOperation,
  ProviderId,
  ReasoningBlock,
} from '@proteus-int/contracts';

// Prepared beside the legacy adapter until all TypeScript consumers switch together.
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

export function decodeLegacyCliTrace(value: unknown): CliWireTrace {
  const source = fields(value, [
    'provider',
    'reasoning-ms',
    'wall-time-ms',
    'active-reasoning-ms',
    'requested-model',
    'resolved-model',
    'requested-effort',
    'applied-effort',
    'cli-version',
    'terminal-ms',
    'summary',
    'reasoning',
    'reasoning-truncated',
  ]);
  return {
    provider: choice(source['provider'], ['codex', 'kimi'] as const),
    reasoningMs: duration(source['reasoning-ms']),
    terminalMs: duration(source['terminal-ms']),
    summary: text(source['summary']),
    ...('wall-time-ms' in source ? { wallTimeMs: duration(source['wall-time-ms']) } : {}),
    ...('active-reasoning-ms' in source
      ? { activeReasoningMs: unknownDuration(source['active-reasoning-ms']) }
      : {}),
    ...('requested-model' in source ? { requestedModel: text(source['requested-model']) } : {}),
    ...('resolved-model' in source
      ? { resolvedModel: nullableText(source['resolved-model']) }
      : {}),
    ...('requested-effort' in source
      ? { requestedEffort: effort(source['requested-effort']) }
      : {}),
    ...('applied-effort' in source
      ? {
          appliedEffort:
            source['applied-effort'] === null ? null : effort(source['applied-effort']),
        }
      : {}),
    ...('cli-version' in source ? { cliVersion: nullableText(source['cli-version']) } : {}),
    ...('reasoning' in source ? { reasoning: reasoningBlocks(source['reasoning']) } : {}),
    ...('reasoning-truncated' in source
      ? { reasoningTruncated: boolean(source['reasoning-truncated']) }
      : {}),
  };
}

export function decodeLegacyCliResult(value: unknown): CliWireResult {
  const source = fields(value, ['answer', 'trace']);
  return { answer: text(source['answer']), trace: decodeLegacyCliTrace(source['trace']) };
}

export function decodeLegacyCliEvent(value: unknown): CliWireEvent {
  const source = fields(value, ['id', 'kind', 'elapsed-ms', 'reasoning']);
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
    elapsedMs: duration(source['elapsed-ms']),
    ...('reasoning' in source ? { reasoning: reasoningBlock(source['reasoning']) } : {}),
  };
}

export function encodeLegacyCliRequest(value: unknown): LegacyCliRequest {
  const source = fields(value, [
    'id',
    'provider',
    'model',
    'task',
    'systemPrompt',
    'effort',
    'operation',
    'sourceContext',
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
  return {
    id,
    provider: choice(source['provider'], ['codex', 'kimi'] as const),
    model,
    task: text(source['task'], 200_000),
    'system-prompt': text(source['systemPrompt'], 40_000),
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
    ...('sourceContext' in source ? { 'source-context': boolean(source['sourceContext']) } : {}),
  };
}
