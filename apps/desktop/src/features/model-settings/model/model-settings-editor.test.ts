import type * as SolidClient from 'solid-js';
import { createRoot, createSignal } from 'solid-js';
import { expect, it, vi } from 'vitest';
import { createModelSettings } from '@proteus-int/intelligence';
import { createModelSettingsEditor } from './model-settings-editor';
vi.mock('solid-js', () => vi.importActual<typeof SolidClient>('solid-js/dist/solid.js'));

function fixture(hasSources: boolean) {
  return createRoot((dispose) => {
    const [applied, apply] = createSignal(createModelSettings());
    const onChange = vi.fn(apply);
    const editor = createModelSettingsEditor({
      settings: applied,
      hasSources: () => hasSources,
      onChange,
    });
    return { editor, applied, onChange, dispose };
  });
}
it('only applies the global provider after the explicit action', () => {
  const { editor, applied, onChange, dispose } = fixture(false);
  editor.selectGlobalProvider('kimi');
  expect(applied().components.stalker).toBe('codex');
  expect(onChange).not.toHaveBeenCalled();
  editor.applyGlobal();
  expect(Object.values(applied().components)).toEqual(['kimi', 'kimi', 'kimi', 'kimi']);
  dispose();
});
it('retains the full pending STALKER alias and every grant until confirmation', () => {
  const { editor, applied, onChange, dispose } = fixture(true);
  for (const value of ['first', 'first ', 'first second']) editor.changeAlias('stalker', value);
  editor.changeProvider('person', 'kimi');
  expect(editor.settings().aliases.stalker).toBe('first second');
  expect(applied()).toEqual(createModelSettings());
  expect(onChange).not.toHaveBeenCalled();
  editor.confirm();
  expect(applied().aliases.stalker).toBe('first second');
  expect(applied().components.person).toBe('kimi');
  expect(onChange).toHaveBeenCalledOnce();
  expect(editor.pending()).toBeUndefined();
  dispose();
});
it('discarding a proposed STALKER change leaves applied settings intact', () => {
  const { editor, applied, onChange, dispose } = fixture(true);
  editor.selectGlobalProvider('kimi');
  editor.applyGlobal();
  expect(editor.pending()).toBeDefined();
  editor.discard();
  expect(editor.settings()).toEqual(applied());
  expect(onChange).not.toHaveBeenCalled();
  editor.changeProvider('person', 'kimi');
  expect(applied().components.person).toBe('kimi');
  expect(editor.pending()).toBeUndefined();
  dispose();
});
