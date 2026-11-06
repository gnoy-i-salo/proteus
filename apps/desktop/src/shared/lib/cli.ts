import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type {
  IntelligenceResult,
  IntelligenceTrace,
  ProviderId,
  ReasoningBlock,
} from '@proteus-int/contracts';
import { IntelligenceRunError } from '@proteus-int/intelligence';
import type { IntelligenceProvider } from '@proteus-int/intelligence';
import { isDesktop } from './window-controls';
export interface CliStatus {
  provider: ProviderId;
  available: boolean;
  path: string | null;
}
export interface CliEvent {
  id: string;
  kind: string;
  'elapsed-ms': number;
  reasoning?: ReasoningBlock;
}
export async function cliStatus(): Promise<CliStatus[]> {
  return isDesktop() ? invoke('cli_status') : [];
}
export function cliProviders(
  onEvent: (event: CliEvent) => void,
): Record<ProviderId, IntelligenceProvider> {
  const adapter = (id: ProviderId): IntelligenceProvider => ({
    id,
    async run(request, signal) {
      if (!isDesktop()) throw new Error('Запуск CLI доступен в настольном приложении PROTEUS.');
      signal?.throwIfAborted();
      const runId = crypto.randomUUID();
      const started = performance.now();
      let elapsed = 0;
      let returned: IntelligenceTrace | undefined;
      const reasoning: ReasoningBlock[] = [];
      let truncated = false;
      const unlisten = await listen<CliEvent>('cli-progress', ({ payload }) => {
        if (payload.id !== runId) return;
        if (Number.isFinite(payload['elapsed-ms']))
          elapsed = Math.max(elapsed, payload['elapsed-ms']);
        const block = payload.reasoning;
        if (
          block &&
          ['summary', 'thinking'].includes(block.kind) &&
          typeof block.text === 'string'
        ) {
          const index = reasoning.findIndex((item) => item.id === block.id);
          const encoder = new TextEncoder();
          const used = reasoning.reduce(
            (total, item, i) => total + (i === index ? 0 : encoder.encode(item.text).length),
            0,
          );
          const available = Math.max(0, 48 * 1024 - used);
          const bytes = encoder.encode(block.text);
          let end = Math.min(bytes.length, available);
          while (end > 0 && end < bytes.length && (bytes[end]! & 0xc0) === 0x80) end--;
          truncated ||= end < bytes.length || (index < 0 && reasoning.length >= 64);
          if (index >= 0 || reasoning.length < 64) {
            const snapshot = { ...block, text: new TextDecoder().decode(bytes.subarray(0, end)) };
            if (index < 0) reasoning.push(snapshot);
            else reasoning[index] = snapshot;
          }
        }
        onEvent(payload);
      });
      const cancel = () => {
        void invoke('cancel_cli', { id: runId });
      };
      signal?.addEventListener('abort', cancel, { once: true });
      try {
        signal?.throwIfAborted();
        const result = await invoke<IntelligenceResult>('run_cli', {
          request: { ...request, id: runId, provider: id, model: request.model ?? '' },
        });
        returned = result.trace;
        signal?.throwIfAborted();
        return result;
      } catch (failure) {
        const message = failure instanceof Error ? failure.message : String(failure);
        const wallTime = returned
          ? (returned['wall-time-ms'] ?? returned['reasoning-ms'])
          : Math.max(elapsed, performance.now() - started);
        throw new IntelligenceRunError(
          message,
          {
            provider: id,
            'requested-model': request.model ?? '',
            'requested-effort': request.effort,
            'resolved-model': null,
            'applied-effort': null,
            'cli-version': null,
            'terminal-ms': 0,
            reasoning,
            'reasoning-truncated': truncated,
            ...returned,
            'wall-time-ms': wallTime,
            'reasoning-ms': wallTime,
            'active-reasoning-ms': null,
            summary: `Запрос не завершён: ${message}`,
          },
          { cause: failure },
        );
      } finally {
        signal?.removeEventListener('abort', cancel);
        unlisten();
      }
    },
  });
  return { codex: adapter('codex'), kimi: adapter('kimi') };
}
export async function saveExperiment(data: unknown): Promise<string> {
  if (!isDesktop()) return '';
  return invoke('save_experiment', { id: crypto.randomUUID(), data: JSON.stringify(data) });
}
