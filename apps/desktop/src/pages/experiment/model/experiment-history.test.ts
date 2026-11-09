import type * as SolidClient from 'solid-js';
import { createRoot } from 'solid-js';
import { expect, it, vi } from 'vitest';
import type {
  SavedExperimentPage,
  SavedExperimentText,
} from '../../../shared/lib/experiment-history';
import { createExperimentHistory } from './experiment-history';
import type { ExperimentHistoryPort } from './experiment-history';
vi.mock('solid-js', () => vi.importActual<typeof SolidClient>('solid-js/dist/solid.js'));

function page(ids = ['a', 'b'], nextOffset: number | null = null): SavedExperimentPage {
  return {
    schemaVersion: 1,
    entries: ids.map((id) => ({ id, ok: true, error: null })),
    matched: nextOffset ?? ids.length,
    nextOffset,
    scanTruncated: false,
  };
}
function saved(id: string, text = ` exact ${id}\r\né😀 `): SavedExperimentText {
  return { schemaVersion: 1, id, text };
}
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: Error) => void;
  const promise = new Promise<T>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}
function setup() {
  const port = {
    list: vi.fn<ExperimentHistoryPort['list']>().mockResolvedValue(page()),
    read: vi
      .fn<ExperimentHistoryPort['read']>()
      .mockImplementation((id) => Promise.resolve(saved(id))),
  };
  const decode = vi.fn((text: string) => ({ answer: text }));
  let dispose!: () => void;
  const model = createRoot((stop) => {
    dispose = stop;
    return createExperimentHistory(decode, port);
  });
  return { model, dispose, port, decode };
}

it('does no IO on construction and retains exact content only after explicit open', async () => {
  const f = setup();
  expect(f.port.list).not.toHaveBeenCalled();
  expect(f.port.read).not.toHaveBeenCalled();
  await f.model.refresh();
  expect(f.model.page()).toEqual(page());
  expect(f.port.list).toHaveBeenCalledWith(0, 50, expect.any(AbortSignal));
  await f.model.open('a');
  expect(f.decode).toHaveBeenCalledExactlyOnceWith(saved('a').text);
  expect(f.model.opened()).toEqual({
    id: 'a',
    sourceText: saved('a').text,
    result: { answer: saved('a').text },
  });
  f.dispose();
});

it('replaces pages without promising a stable merged directory snapshot', async () => {
  const f = setup();
  const ids = Array.from({ length: 50 }, (_, index) => `a-${String(index).padStart(2, '0')}`);
  const first = { ...page(ids, 50), matched: 51, scanTruncated: true };
  f.port.list.mockResolvedValueOnce(first).mockResolvedValueOnce({ ...page(['z']), matched: 51 });
  await f.model.refresh();
  expect(f.model.page()?.scanTruncated).toBe(true);
  await f.model.nextPage();
  expect(f.model.offset()).toBe(50);
  expect(f.model.page()?.entries.map((e) => e.id)).toEqual(['z']);
  expect(f.port.list.mock.calls[1]?.slice(0, 2)).toEqual([50, 50]);
  await f.model.nextPage();
  expect(f.port.list).toHaveBeenCalledTimes(2);
  await f.model.previousPage();
  expect(f.model.offset()).toBe(0);
  f.dispose();
});

it('discards stale list results and errors after a newer refresh', async () => {
  const f = setup();
  const old = deferred<SavedExperimentPage>();
  f.port.list.mockReturnValueOnce(old.promise);
  const first = f.model.refresh();
  const signal = f.port.list.mock.calls[0]?.[2];
  await f.model.refresh();
  expect(signal?.aborted).toBe(true);
  old.reject(new Error('obsolete error'));
  await first;
  expect(f.model.page()).toEqual(page());
  expect(f.model.listError()).toBe('');
  expect(f.model.loading()).toBe(false);
  f.dispose();
});

it('keeps the last page and opened result when refresh or decoding fails', async () => {
  const f = setup();
  await f.model.refresh();
  await f.model.open('a');
  const previous = f.model.opened();
  f.port.list.mockRejectedValueOnce(new Error('unavailable'));
  await f.model.refresh();
  expect(f.model.page()).toEqual(page());
  expect(f.model.listError()).not.toBe('');
  f.decode.mockImplementationOnce(() => {
    throw new Error('unsupported schema');
  });
  await f.model.open('b');
  expect(f.model.opened()).toBe(previous);
  expect(f.model.openError()).not.toBe('');
  expect(f.model.openingId()).toBeUndefined();
  f.dispose();
});

it('only the latest explicit open can replace the selected result', async () => {
  const f = setup();
  await f.model.refresh();
  const old = deferred<SavedExperimentText>();
  f.port.read.mockReturnValueOnce(old.promise);
  const first = f.model.open('a');
  expect(f.model.openingId()).toBe('a');
  const signal = f.port.read.mock.calls[0]?.[1];
  await f.model.open('b');
  old.resolve(saved('a'));
  await first;
  expect(signal?.aborted).toBe(true);
  expect(f.decode).toHaveBeenCalledTimes(1);
  expect(f.model.opened()?.id).toBe('b');
  f.dispose();
});

it('never reads unlisted or known unavailable records', async () => {
  const f = setup();
  f.port.list.mockResolvedValue({
    ...page(['a']),
    entries: [{ id: 'a', ok: false, error: 'missingResult' }],
  });
  await f.model.refresh();
  await f.model.open('a');
  expect(f.model.openError()).not.toBe('');
  await f.model.open('../private');
  expect(f.port.read).not.toHaveBeenCalled();
  expect(f.decode).not.toHaveBeenCalled();
  f.dispose();
});

it('cancel retains the current result and ignores a late read failure', async () => {
  const f = setup();
  await f.model.refresh();
  await f.model.open('a');
  const old = deferred<SavedExperimentText>();
  f.port.read.mockReturnValueOnce(old.promise);
  const opening = f.model.open('b');
  const signal = f.port.read.mock.calls[1]?.[1];
  f.model.cancel();
  expect(signal?.aborted).toBe(true);
  expect(f.model.openingId()).toBeUndefined();
  old.reject(new Error('late read failure'));
  await opening;
  expect(f.model.openError()).toBe('');
  expect(f.model.opened()?.id).toBe('a');
  f.dispose();
});

it('clear cancels a pending replacement before dismissing the opened result', async () => {
  const f = setup();
  await f.model.refresh();
  await f.model.open('a');
  const old = deferred<SavedExperimentText>();
  f.port.read.mockReturnValueOnce(old.promise);
  const opening = f.model.open('b');
  f.model.clearOpened();
  old.resolve(saved('b'));
  await opening;
  expect(f.model.opened()).toBeUndefined();
  expect(f.decode).toHaveBeenCalledTimes(1);
  f.dispose();
});

it('disposal aborts both requests and prevents further IO or decoding', async () => {
  const f = setup();
  await f.model.refresh();
  const list = deferred<SavedExperimentPage>();
  const read = deferred<SavedExperimentText>();
  f.port.list.mockReturnValueOnce(list.promise);
  f.port.read.mockReturnValueOnce(read.promise);
  const refreshing = f.model.refresh();
  const opening = f.model.open('a');
  const listSignal = f.port.list.mock.calls[1]?.[2];
  const readSignal = f.port.read.mock.calls[0]?.[1];
  f.dispose();
  expect(listSignal?.aborted).toBe(true);
  expect(readSignal?.aborted).toBe(true);
  list.resolve(page(['late']));
  read.resolve(saved('a'));
  await Promise.all([refreshing, opening]);
  await f.model.refresh();
  await f.model.open('b');
  expect(f.port.list).toHaveBeenCalledTimes(2);
  expect(f.port.read).toHaveBeenCalledTimes(1);
  expect(f.decode).not.toHaveBeenCalled();
});
