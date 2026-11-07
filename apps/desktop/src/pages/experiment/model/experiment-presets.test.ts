import type * as SolidClient from 'solid-js';
import { createRoot, createSignal } from 'solid-js';
import { expect, it, vi } from 'vitest';
import type { ExperimentInput } from '@proteus-int/contracts';
import { serializePersonaPreset } from '@proteus-int/pipeline';
import { createExperimentPresets } from './experiment-presets';
import type { ExperimentPresetsPort } from './experiment-presets';
vi.mock('solid-js', () => vi.importActual<typeof SolidClient>('solid-js/dist/solid.js'));
const input: ExperimentInput = {
  task: 'Exact\r\n👩🏽‍🔬',
  people: [
    { label: 'A', context: ' Exact A ', publicUrls: [] },
    { label: 'B', context: ' Exact B ', publicUrls: ['https://example.test'] },
  ],
};
function setup(desktop = true) {
  const port = {
    isDesktop: () => desktop,
    importPreset: vi.fn<ExperimentPresetsPort['importPreset']>().mockResolvedValue(null),
    exportPreset: vi.fn<ExperimentPresetsPort['exportPreset']>().mockResolvedValue(undefined),
    listenPresets: vi.fn<ExperimentPresetsPort['listenPresets']>().mockResolvedValue(() => {}),
  };
  const applyInput = vi.fn();
  const clearSourceGrants = vi.fn();
  const onApplied = vi.fn();
  const reportError = vi.fn();
  const openBrowserFile = vi.fn();
  let dispose!: () => void;
  const owned = createRoot((stop) => {
    dispose = stop;
    const [running, setRunning] = createSignal(false);
    const model = createExperimentPresets(
      {
        running,
        currentInput: () => input,
        applyInput,
        clearSourceGrants,
        onApplied,
        reportError,
        openBrowserFile,
      },
      port,
    );
    return { model, setRunning };
  });
  return {
    ...owned,
    dispose,
    port,
    applyInput,
    clearSourceGrants,
    onApplied,
    reportError,
    openBrowserFile,
  };
}
it('defers incoming presets during execution and applies them only after explicit selection', () => {
  const fixture = setup();
  fixture.setRunning(true);
  fixture.model.receivePreset(serializePersonaPreset(input));
  expect(fixture.model.pendingPreset()).toEqual(input);
  expect(fixture.applyInput).not.toHaveBeenCalled();
  fixture.setRunning(false);
  expect(fixture.applyInput).not.toHaveBeenCalled();
  fixture.model.applyPending();
  expect(fixture.applyInput).toHaveBeenCalledExactlyOnceWith(input);
  expect(fixture.clearSourceGrants).toHaveBeenCalledOnce();
  expect(fixture.onApplied).toHaveBeenCalledOnce();
  expect(fixture.model.pendingPreset()).toBeUndefined();
  fixture.dispose();
});
it('keeps invalid or cancelled imports from replacing current input', async () => {
  const fixture = setup();
  await fixture.model.openPreset();
  fixture.model.receivePreset('{broken');
  expect(fixture.applyInput).not.toHaveBeenCalled();
  expect(fixture.clearSourceGrants).not.toHaveBeenCalled();
  expect(fixture.reportError).toHaveBeenCalledWith(expect.stringContaining('JSON'));
  fixture.dispose();
});
it('validates browser file selection before reading and keeps the exact serialized content', async () => {
  const fixture = setup(false);
  await fixture.model.openPreset();
  expect(fixture.openBrowserFile).toHaveBeenCalledOnce();
  expect(fixture.port.importPreset).not.toHaveBeenCalled();
  const text = vi.fn().mockResolvedValue(serializePersonaPreset(input));
  await fixture.model.readBrowserFile({ name: 'bad.txt', size: 20, text });
  await fixture.model.readBrowserFile({ name: 'large.proteus', size: 131073, text });
  expect(text).not.toHaveBeenCalled();
  await fixture.model.readBrowserFile({ name: 'input.PROTEUS', size: 100, text });
  expect(fixture.applyInput).toHaveBeenCalledWith(input);
  await fixture.model.savePreset();
  expect(fixture.port.exportPreset).toHaveBeenCalledWith(serializePersonaPreset(input));
  fixture.dispose();
});
it('aborts its subscription on disposal and can dismiss a deferred preset without applying it', () => {
  const fixture = setup();
  const signal = fixture.port.listenPresets.mock.calls[0]?.[2];
  expect(signal?.aborted).toBe(false);
  fixture.setRunning(true);
  fixture.model.receivePreset(serializePersonaPreset(input));
  fixture.model.dismissPending();
  expect(fixture.model.pendingPreset()).toBeUndefined();
  expect(fixture.applyInput).not.toHaveBeenCalled();
  fixture.dispose();
  expect(signal?.aborted).toBe(true);
});
