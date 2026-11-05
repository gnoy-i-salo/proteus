import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { IntelligenceResult, ProviderId } from '@proteus-int/contracts';
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
      const unlisten = await listen<CliEvent>('cli-progress', ({ payload }) => {
        if (payload.id === runId) onEvent(payload);
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
        signal?.throwIfAborted();
        return result;
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
