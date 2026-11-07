import { describe, expect, it, vi } from 'vitest';
import type * as SolidClient from 'solid-js';
vi.mock('solid-js', () => vi.importActual<typeof SolidClient>('solid-js/dist/solid.js'));
import { createRoot, createSignal } from 'solid-js';
import { createTextWorkspace } from './text-workspace-model';

describe('document navigation', () => {
  it('keeps global range/direction across reflow and replaces its whole suffix', () =>
    createRoot((dispose) => {
      const [value, change] = createSignal('Начало  👩🏽‍🔬\nдлинный выбранный хвост  конец');
      const model = createTextWorkspace({ value, change, resetKey: () => 1 });
      model.setLayout({ width: 100, rows: 4, measure: (text) => text.length });
      model.select({ start: 8, end: value().length - 7, direction: 'backward' }, true);
      const selected = model.selectedText();
      model.setLayout({ width: 8, rows: 2, measure: (text) => text.length });
      expect(model.projected().continues).toBe(true);
      expect(model.selection().direction).toBe('backward');
      expect(model.selectedText()).toBe(selected);
      model.replace('точно ✨\n');
      expect(value()).toBe('Начало  точно ✨\n  конец');
      dispose();
    }));
  it('returns to the exact pre-search place and selection after resize', () =>
    createRoot((dispose) => {
      const model = createTextWorkspace({
        value: () => 'A '.repeat(100) + 'НАЙТИ',
        resetKey: () => 1,
      });
      model.setAnchor(30);
      model.select({ start: 30, end: 40, direction: 'backward' });
      model.search('НАЙТИ');
      model.find();
      expect(model.selection().start).toBe(200);
      model.setLayout({ width: 50, rows: 8, measure: (text) => text.length });
      model.returnToPlace();
      expect(model.anchor()).toBe(30);
      expect(model.selection()).toEqual({ start: 30, end: 40, direction: 'backward' });
      dispose();
    }));
  it('Next at the last page cannot add text or an empty page', () =>
    createRoot((dispose) => {
      const change = vi.fn();
      const model = createTextWorkspace({ value: () => 'Exact\n', change, resetKey: () => 1 });
      model.navigate(1000);
      expect(model.pageIndex()).toBe(model.pages().length - 1);
      expect(change).not.toHaveBeenCalled();
      expect(model.pages().every((page) => page.end > page.start)).toBe(true);
      dispose();
    }));
});
it('handles a non-cancellable input replacement over a projected range without dropping its suffix', () =>
  createRoot((dispose) => {
    const [value, change] = createSignal('ABCDEFGHIJKLMN');
    const model = createTextWorkspace({ value, change, resetKey: () => 1 });
    model.setLayout({ width: 5, rows: 1, measure: (text) => text.length });
    model.select({ start: 2, end: 12, direction: 'backward' }, true);
    model.editPage('AB漢', 3, 3, 'none');
    expect(value()).toBe('AB漢MN');
    dispose();
  }));
it('replaces the selected original CRLF span even when neighboring DOM text is identical', () =>
  createRoot((dispose) => {
    const [value, change] = createSignal('A\r\nA\n');
    const model = createTextWorkspace({ value, change, resetKey: () => 1 });
    model.setLayout({ width: 50, rows: 10, measure: (text) => text.length });
    model.select({ start: 0, end: 3, direction: 'forward' });
    model.editPage('A\n', 0, 0, 'none');
    expect(value()).toBe('A\n');
    dispose();
  }));
