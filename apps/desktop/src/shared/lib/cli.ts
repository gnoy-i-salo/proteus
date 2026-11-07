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
import {
  decodeCliEvent,
  decodeCliResult,
  encodeCliRequest,
  decodeCliStatuses,
  decodeSavedExperimentPath,
} from './cli-wire-codec';
import type { CliWireEvent, CliWireStatus } from './cli-wire-codec';
export type CliStatus = CliWireStatus;
export type CliEvent = CliWireEvent;

export async function cliStatus(): Promise<CliStatus[]> {
  return isDesktop() ? invoke<unknown>('cli_status').then(decodeCliStatuses) : [];
}
export function cliProviders(
  onEvent: (event: CliEvent) => void,
): Record<ProviderId, IntelligenceProvider> {
  const adapter = (id: ProviderId): IntelligenceProvider => ({
    id,
    async run(request, signal) {
      if (!isDesktop()) throw new Error('Запуск CLI доступен в настольном приложении PROTEUS.');
      signal?.throwIfAborted();
      const { invocationId, ...nativeRequest } = request;
      const runId = invocationId ?? crypto.randomUUID();
      const started = performance.now();
      let elapsed = 0;
      let returned: IntelligenceTrace | undefined;
      const reasoning: ReasoningBlock[] = [];
      let truncated = false;
      let streamFailure: unknown;
      const unlisten = await listen<unknown>('cli-progress', ({ payload: raw }) => {
        if (!raw || typeof raw !== 'object' || !('id' in raw) || raw.id !== runId) return;
        let payload: CliEvent;
        try {
          let bounded: unknown = raw;
          // Preserve the existing public-text cap before strict wire validation.
          // This only shortens the known reasoning text field; no other content is relabelled.
          if (
            'reasoning' in raw &&
            raw.reasoning &&
            typeof raw.reasoning === 'object' &&
            'text' in raw.reasoning &&
            typeof raw.reasoning.text === 'string'
          ) {
            const bytes = new TextEncoder().encode(raw.reasoning.text);
            let end = Math.min(bytes.length, 48 * 1024);
            while (end > 0 && end < bytes.length && (bytes[end]! & 0xc0) === 0x80) end--;
            if (end < bytes.length) {
              truncated = true;
              bounded = {
                ...raw,
                reasoning: {
                  ...raw.reasoning,
                  text: new TextDecoder().decode(bytes.subarray(0, end)),
                },
              };
            }
          }
          payload = decodeCliEvent(bounded);
        } catch (failure) {
          streamFailure = failure;
          void invoke('cancel_cli', { id: runId }).catch(() => undefined);
          return;
        }
        if (Number.isFinite(payload.elapsedMs)) elapsed = Math.max(elapsed, payload.elapsedMs);
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
        const rawResult = await invoke<unknown>('run_cli', {
          request: encodeCliRequest({
            ...nativeRequest,
            id: runId,
            provider: id,
            model: request.model ?? '',
          }),
        });
        if (streamFailure) throw streamFailure;
        const result: IntelligenceResult = decodeCliResult(rawResult);
        returned = result.trace;
        signal?.throwIfAborted();
        return result;
      } catch (failure) {
        const message = failure instanceof Error ? failure.message : String(failure);
        const wallTime = returned
          ? (returned.wallTimeMs ?? returned.reasoningMs)
          : Math.max(elapsed, performance.now() - started);
        throw new IntelligenceRunError(
          message,
          {
            provider: id,
            requestedModel: request.model ?? '',
            requestedEffort: request.effort,
            resolvedModel: null,
            appliedEffort: null,
            cliVersion: null,
            terminalMs: 0,
            reasoning,
            reasoningTruncated: truncated,
            ...returned,
            wallTimeMs: wallTime,
            reasoningMs: wallTime,
            activeReasoningMs: null,
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
  return invoke<unknown>('save_experiment', {
    id: crypto.randomUUID(),
    data: JSON.stringify(data),
  }).then(decodeSavedExperimentPath);
}
