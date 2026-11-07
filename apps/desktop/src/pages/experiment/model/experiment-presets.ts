import { batch, createSignal, onCleanup, onMount } from 'solid-js';
import type { ExperimentInput } from '@proteus-int/contracts';
import { parsePersonaPreset, serializePersonaPreset } from '@proteus-int/pipeline';
import { importPreset, exportPreset, listenPresets } from '../../../shared/lib/presets';
import { isDesktop } from '../../../shared/lib/window-controls';

export interface ExperimentPresetsPort {
  readonly importPreset: typeof importPreset;
  readonly exportPreset: typeof exportPreset;
  readonly listenPresets: typeof listenPresets;
  readonly isDesktop: typeof isDesktop;
}
export function createExperimentPresets(
  options: {
    readonly running: () => boolean;
    readonly currentInput: () => ExperimentInput;
    readonly applyInput: (input: ExperimentInput) => void;
    readonly clearSourceGrants: () => void;
    readonly onApplied: () => void;
    readonly reportError: (message: string) => void;
    readonly openBrowserFile: () => void;
  },
  port: ExperimentPresetsPort = { importPreset, exportPreset, listenPresets, isDesktop },
) {
  const [pendingPreset, setPendingPreset] = createSignal<ExperimentInput>();
  const applyPreset = (input: ExperimentInput) => {
    options.clearSourceGrants();
    batch(() => {
      options.applyInput(input);
      options.onApplied();
      options.reportError('');
    });
  };
  const receivePreset = (data: string) => {
    try {
      const input = parsePersonaPreset(data);
      if (options.running()) setPendingPreset(input);
      else applyPreset(input);
    } catch (failure) {
      options.reportError(String(failure));
    }
  };
  onMount(() => {
    const controller = new AbortController();
    void port
      .listenPresets(
        receivePreset,
        (failure) => options.reportError(failure.message),
        controller.signal,
      )
      .catch((failure) => {
        if (!controller.signal.aborted) options.reportError(String(failure));
      });
    onCleanup(() => controller.abort());
  });
  const openPreset = async () => {
    if (!port.isDesktop()) {
      options.openBrowserFile();
      return;
    }
    try {
      const data = await port.importPreset();
      if (data !== null) receivePreset(data);
    } catch (failure) {
      options.reportError(String(failure));
    }
  };
  const savePreset = async () => {
    try {
      await port.exportPreset(serializePersonaPreset(options.currentInput()));
    } catch (failure) {
      options.reportError(String(failure));
    }
  };
  const readBrowserFile = async (file: Pick<File, 'name' | 'size' | 'text'> | undefined) => {
    if (!file) return;
    if (!file.name.toLowerCase().endsWith('.proteus') || file.size > 131072) {
      options.reportError('Выберите файл .proteus до 128 КиБ.');
      return;
    }
    try {
      receivePreset(await file.text());
    } catch (failure) {
      options.reportError(String(failure));
    }
  };
  const applyPending = () => {
    const input = pendingPreset();
    if (input) applyPreset(input);
    setPendingPreset(undefined);
  };
  return {
    pendingPreset,
    receivePreset,
    openPreset,
    savePreset,
    readBrowserFile,
    applyPending,
    dismissPending: () => setPendingPreset(undefined),
  };
}
