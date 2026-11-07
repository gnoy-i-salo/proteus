import { createEffect, createMemo, createSignal, on, untrack } from 'solid-js';
import {
  applyPageEdit,
  domOffset,
  sourceOffset,
  documentPages,
  findTextMatches,
  pageForOffset,
  projectSelection,
  replaceSelection,
  textOutline,
} from '../../lib/text-document';
import type { TextLayout, TextSelection } from '../../lib/text-document';

export interface TextWorkspacePosition {
  readonly anchor: number;
  readonly selection: TextSelection;
  readonly query: string;
  readonly match: number;
  readonly bookmark?: { anchor: number; selection: TextSelection };
}

export function createTextWorkspace(options: {
  value: () => string;
  resetKey: () => string | number | undefined;
  change?: (value: string) => void;
  interact?: () => void;
  position?: () => TextWorkspacePosition | undefined;
  onPosition?: (position: TextWorkspacePosition) => void;
}) {
  const [layout, setLayout] = createSignal<TextLayout>({
    width: 20,
    rows: 1,
    measure: (value) => [...value].length,
  });
  const [anchor, setAnchor] = createSignal(0);
  const [selection, setSelection] = createSignal<TextSelection>({
    start: 0,
    end: 0,
    direction: 'none',
  });
  const [query, setQuery] = createSignal('');
  const [match, setMatch] = createSignal(-1);
  const [bookmark, setBookmark] = createSignal<{ anchor: number; selection: TextSelection }>();
  const pages = createMemo(() => documentPages(options.value(), layout()));
  const pageIndex = () => pageForOffset(pages(), Math.min(anchor(), options.value().length));
  const page = () => pages()[pageIndex()]!;
  const visible = () => options.value().slice(page().start, page().end);
  const projected = () => projectSelection(selection(), page());
  const matches = createMemo(() => findTextMatches(options.value(), query()));
  const outline = createMemo(() => textOutline(options.value()));
  const select = (next: TextSelection, reveal = false) => {
    setSelection(next);
    if (reveal) setAnchor(next.direction === 'backward' ? next.start : next.end);
  };
  const navigate = (index: number) => {
    const next = pages()[Math.max(0, Math.min(pages().length - 1, index))]!;
    setAnchor(next.start);
    select({ start: next.start, end: next.start, direction: 'none' });
    options.interact?.();
  };
  const replace = (inserted: string) => {
    const next = replaceSelection(options.value(), selection(), inserted);
    options.change?.(next.text);
    select(next.selection, true);
  };
  const editPage = (
    value: string,
    start: number,
    end: number,
    direction: TextSelection['direction'],
  ) => {
    const original = page();
    const projected = projectSelection(selection(), original);
    if (selection().end > selection().start) {
      const before = visible().replace(/\r\n?/g, '\n');
      const localStart = domOffset(visible(), projected.start);
      const localEnd = domOffset(visible(), projected.end);
      const prefix = before.slice(0, localStart);
      const suffix = before.slice(localEnd);
      if (
        value.startsWith(prefix) &&
        value.endsWith(suffix) &&
        value.length >= prefix.length + suffix.length
      ) {
        replace(value.slice(prefix.length, value.length - suffix.length));
        return;
      }
    }
    const edited = applyPageEdit(visible(), value);
    const next =
      options.value().slice(0, original.start) + edited + options.value().slice(original.end);
    options.change?.(next);
    select(
      {
        start: original.start + sourceOffset(edited, start),
        end: original.start + sourceOffset(edited, end),
        direction,
      },
      true,
    );
  };
  const search = (value: string) => {
    if (!bookmark()) setBookmark({ anchor: anchor(), selection: selection() });
    setQuery(value);
    setMatch(-1);
  };
  const find = (step = 1) => {
    if (!matches().length) return;
    const index = (match() + step + matches().length) % matches().length;
    const found = matches()[index]!;
    setMatch(index);
    select({ ...found, direction: 'forward' }, false);
    setAnchor(found.start);
    options.interact?.();
  };
  const returnToPlace = () => {
    const saved = bookmark();
    if (saved) {
      setAnchor(saved.anchor);
      setSelection(saved.selection);
    }
    setBookmark(undefined);
    setQuery('');
    setMatch(-1);
  };
  createEffect(
    on(options.resetKey, () => {
      const saved = options.position?.();
      setAnchor(saved?.anchor ?? 0);
      setSelection(saved?.selection ?? { start: 0, end: 0, direction: 'none' });
      setBookmark(saved?.bookmark ? { ...saved.bookmark } : undefined);
      setQuery(saved?.query ?? '');
      setMatch(saved?.match ?? -1);
    }),
  );
  createEffect(() => {
    const position = {
      anchor: anchor(),
      selection: selection(),
      query: query(),
      match: match(),
      ...(bookmark() ? { bookmark: { ...bookmark()! } } : {}),
    };
    // A new stage changes the callback's owner. It must not publish the previous
    // stage's coordinates before reset has restored the new stage's position.
    untrack(() => options.onPosition?.(position));
  });
  return {
    pages,
    pageIndex,
    page,
    visible,
    projected,
    selection,
    select,
    navigate,
    replace,
    editPage,
    layout,
    setLayout,
    anchor,
    setAnchor,
    query,
    search,
    matches,
    match,
    find,
    bookmark,
    returnToPlace,
    outline,
    selectedText: () => options.value().slice(selection().start, selection().end),
  };
}
