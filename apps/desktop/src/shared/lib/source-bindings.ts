import { invoke } from '@tauri-apps/api/core';
import type {
  SourceBindingRequest,
  CreatedSourceBinding,
  SourceAction,
  SourceBindingResult,
} from '@proteus-int/contracts';
import { isDesktop } from './window-controls';
import {
  encodeSourceBindingRequest,
  decodeCreatedSourceBinding,
  encodeBindingId,
  encodeBindingAction,
  decodeSourceBindingResult,
} from './source-binding-wire-codec';

function requireDesktop(): void {
  if (!isDesktop())
    throw new Error('Привязки источников доступны в настольном приложении PROTEUS.');
}

export async function createSourceBinding(
  request: SourceBindingRequest,
  signal?: AbortSignal,
): Promise<CreatedSourceBinding> {
  requireDesktop();
  signal?.throwIfAborted();
  const selected = encodeSourceBindingRequest(request);
  const result = await invoke<unknown>('create_source_binding', { request: selected });
  signal?.throwIfAborted();
  return decodeCreatedSourceBinding(result, selected);
}

// Signal suppresses late delivery. Native work remains bounded by the source grant;
// cancelling that grant is an explicit host action using its returned grant ID.
export async function sourceBindingAction(
  bindingId: string,
  action: SourceAction,
  signal?: AbortSignal,
): Promise<SourceBindingResult> {
  requireDesktop();
  signal?.throwIfAborted();
  const result = await invoke<unknown>('source_binding_action', {
    bindingId: encodeBindingId(bindingId),
    action: encodeBindingAction(action),
  });
  signal?.throwIfAborted();
  return decodeSourceBindingResult(result);
}
