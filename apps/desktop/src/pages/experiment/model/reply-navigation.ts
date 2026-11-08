import { batch, createEffect, createMemo, createSignal, on } from 'solid-js';
import type { IntelligenceResult, ModelCallLifecycle } from '@proteus-int/contracts';
import type { ExperimentPerson, StageTrace, TrialResult } from '@proteus-int/pipeline';
import { findTextMatches } from '../../../shared/lib/text-document';
import type { TextWorkspacePosition } from '../../../shared/ui/text-workspace/text-workspace-model';
import { classifyTraceStage } from './execution-stage';

export interface ReplyMessageItem {
  readonly index: number;
  readonly number: number;
  readonly total: number;
  readonly text: string;
  readonly published: boolean;
}

const joinSeparator = '\n\n';

function publishedMessages(attempt: IntelligenceResult | null): readonly string[] {
  if (!attempt) return [];
  const messages = attempt.reply?.messages;
  if (messages && messages.length > 0) return messages;
  return attempt.answer ? [attempt.answer] : [];
}

export function replyMessagesForTrial(trial: TrialResult): readonly ReplyMessageItem[] {
  const messages = publishedMessages(trial.attempt);
  const texts =
    messages.length > 0 ? messages : [trial.error?.trim() ? trial.error : 'Ответ не получен.'];
  return texts.map((text, index) => ({
    index,
    number: index + 1,
    total: texts.length,
    text,
    published: messages.length > 0,
  }));
}

function messageStarts(lengths: readonly number[]): number[] {
  const starts: number[] = [];
  let offset = 0;
  for (const length of lengths) {
    starts.push(offset);
    offset += length + joinSeparator.length;
  }
  return starts;
}

function messageIndexForOffset(starts: readonly number[], offset: number): number {
  let index = 0;
  for (let position = 0; position < starts.length; position++) {
    if (starts[position]! <= offset) index = position;
    else break;
  }
  return index;
}

export interface QuoteOccurrence {
  readonly start: number;
  readonly end: number;
  readonly firstMessage: number;
  readonly lastMessage: number;
}

export type QuoteLocation =
  | { readonly kind: 'found'; readonly occurrences: readonly QuoteOccurrence[] }
  | { readonly kind: 'absent' }
  | { readonly kind: 'unavailable' };

// Quotes are literal excerpts of the published joined answer, never normalized.
export function locateQuote(attemptValue: IntelligenceResult | null, quote: string): QuoteLocation {
  const messages = publishedMessages(attemptValue);
  if (!attemptValue || messages.length === 0 || !attemptValue.answer)
    return { kind: 'unavailable' };
  if (messages.join(joinSeparator) !== attemptValue.answer) return { kind: 'unavailable' };
  if (!quote) return { kind: 'absent' };
  const matches = findTextMatches(attemptValue.answer, quote);
  if (matches.length === 0) return { kind: 'absent' };
  const starts = messageStarts(messages.map((message) => message.length));
  const occurrences = matches.map((match) => {
    const lastCharacter = Math.max(match.start, match.end - 1);
    return {
      start: match.start,
      end: match.end,
      firstMessage: messageIndexForOffset(starts, match.start),
      lastMessage: messageIndexForOffset(starts, lastCharacter),
    };
  });
  return { kind: 'found', occurrences };
}

export function occurrenceOverlap(
  occurrence: QuoteOccurrence,
  messageIndex: number,
  messageLengths: readonly number[],
): { start: number; end: number } | undefined {
  const starts = messageStarts(messageLengths);
  const messageStart = starts[messageIndex];
  if (messageStart === undefined) return undefined;
  const messageEnd = messageStart + messageLengths[messageIndex]!;
  const start = Math.max(occurrence.start, messageStart);
  const end = Math.min(occurrence.end, messageEnd);
  if (end <= start) return undefined;
  return { start: start - messageStart, end: end - messageStart };
}

export type CreatingCallResolution =
  | { readonly kind: 'linked'; readonly stage: StageTrace; readonly call: ModelCallLifecycle }
  | { readonly kind: 'unavailable' };

// The published answer is created by exactly one model call. Preferred link is the
// invocationId carried by the result (contract field); it is accepted only when the
// same callId occurs in a stage lifecycle. Without it, the stage is identified by
// the existing public trace classification (solution phase, round, participant) and
// the call must be unique and completed; anything else stays explicitly unavailable.
export function resolveCreatingCall(
  trial: TrialResult,
  stages: readonly StageTrace[],
  personIndex: number | null,
): CreatingCallResolution {
  if (publishedMessages(trial.attempt).length === 0) return { kind: 'unavailable' };
  const callId = trial.attempt?.invocationId;
  if (callId) {
    const matches = stages.flatMap((stage) =>
      (stage.lifecycle?.calls ?? [])
        .filter((call) => call.callId === callId)
        .map((call) => ({ stage, call })),
    );
    const match = matches[0];
    return matches.length === 1 && match?.call.status === 'completed'
      ? { kind: 'linked', ...match }
      : { kind: 'unavailable' };
  }
  const participant =
    personIndex === 0 ? 'a' : personIndex === 1 ? 'b' : personIndex === 2 ? 'control' : null;
  if (participant === null) return { kind: 'unavailable' };
  const matches = stages.filter((stage) => {
    const identity = classifyTraceStage(stage);
    return (
      identity.phase === 'solution' &&
      identity.round === trial.round &&
      identity.participant === participant
    );
  });
  if (matches.length !== 1) return { kind: 'unavailable' };
  const completed = matches[0]!.lifecycle?.calls.filter((call) => call.status === 'completed');
  if (!completed || completed.length !== 1) return { kind: 'unavailable' };
  return { kind: 'linked', stage: matches[0]!, call: completed[0]! };
}

export interface QuoteTarget {
  readonly quote: string;
  readonly round: number;
  readonly occurrence?: number;
}

export interface ReplyQuoteHighlight {
  readonly anchor: number;
  readonly selection: TextWorkspacePosition['selection'];
  readonly query: string;
  readonly match: number;
}

export function createReplyNavigation(options: {
  person: () => ExperimentPerson | undefined;
  personIndex: () => number | null;
  stages: () => readonly StageTrace[];
}) {
  const [round, setRound] = createSignal<number | null>(null);
  const [message, setMessage] = createSignal(0);
  const [quoteTarget, setQuoteTarget] = createSignal<QuoteTarget | undefined>(undefined);
  const [quoteOccurrence, setQuoteOccurrence] = createSignal(0);
  const positions = new Map<string, TextWorkspacePosition>();

  const trials = createMemo(() => options.person()?.trials ?? []);
  const rounds = createMemo(() =>
    [...new Set(trials().map((trial) => trial.round))].sort((a, b) => a - b),
  );
  const unpublishedRounds = createMemo(
    () =>
      new Set(
        trials()
          .filter((trial) => publishedMessages(trial.attempt).length === 0)
          .map((trial) => trial.round),
      ),
  );
  const trialForRound = (value: number | null | undefined) =>
    trials().find((trial) => trial.round === value);
  const activeRound = () => {
    const chosen = round();
    if (chosen !== null && trials().some((trial) => trial.round === chosen)) return chosen;
    return rounds()[0] ?? null;
  };
  const activeTrial = () => trialForRound(activeRound());
  const items = createMemo(() => {
    const trial = activeTrial();
    return trial ? replyMessagesForTrial(trial) : [];
  });
  const activeMessageIndex = () => Math.min(message(), Math.max(0, items().length - 1));
  const current = () => items()[activeMessageIndex()];

  createEffect(
    on(
      () => options.person()?.profile.id,
      () => {
        batch(() => {
          setRound(null);
          setMessage(0);
          setQuoteTarget(undefined);
          setQuoteOccurrence(0);
        });
      },
    ),
  );

  const quoteLocation = createMemo(() => {
    const target = quoteTarget();
    const trial = trialForRound(target?.round);
    if (!target || !trial) return undefined;
    return locateQuote(trial.attempt, target.quote);
  });
  const quoteOccurrences = () => {
    const location = quoteLocation();
    return location?.kind === 'found' ? location.occurrences : [];
  };
  const quoteOccurrenceIndex = () =>
    Math.min(quoteOccurrence(), Math.max(0, quoteOccurrences().length - 1));

  const highlight = createMemo((): ReplyQuoteHighlight | undefined => {
    const target = quoteTarget();
    if (!target || activeRound() !== target.round) return undefined;
    const location = quoteLocation();
    if (location?.kind !== 'found') return undefined;
    const occurrence = location.occurrences[quoteOccurrenceIndex()];
    if (!occurrence) return undefined;
    const lengths = items().map((item) => item.text.length);
    const overlap = occurrenceOverlap(occurrence, activeMessageIndex(), lengths);
    if (!overlap) return undefined;
    const text = current()?.text ?? '';
    const query = text.slice(overlap.start, overlap.end);
    const match = findTextMatches(text, query).findIndex((item) => item.start === overlap.start);
    if (match < 0) return undefined;
    return {
      anchor: overlap.start,
      selection: { start: overlap.start, end: overlap.end, direction: 'forward' },
      query,
      match,
    };
  });

  const positionKey = () =>
    `${options.person()?.profile.id ?? ''}:${activeRound() ?? ''}:${activeMessageIndex()}`;
  const positionFor = (key: string) => positions.get(key);
  const savePosition = (key: string, position: TextWorkspacePosition) => {
    if (highlight()) return;
    positions.set(key, position);
  };

  const selectRound = (value: number) =>
    batch(() => {
      setRound(value);
      setMessage(0);
    });
  const selectMessage = (index: number) => setMessage(Math.max(0, index));
  const stepMessage = (step: -1 | 1) => {
    const next = activeMessageIndex() + step;
    if (next >= 0 && next < items().length) {
      selectMessage(next);
      return;
    }
    const available = rounds();
    const neighbour = available[available.indexOf(activeRound() ?? -1) + step];
    if (neighbour === undefined) return;
    const texts = replyMessagesForTrial(trialForRound(neighbour)!);
    batch(() => {
      setRound(neighbour);
      setMessage(step > 0 ? 0 : Math.max(0, texts.length - 1));
    });
  };

  const revealQuote = (target: QuoteTarget) => {
    const location = locateQuote(trialForRound(target.round)?.attempt ?? null, target.quote);
    const index =
      location.kind === 'found'
        ? Math.min(target.occurrence ?? 0, location.occurrences.length - 1)
        : 0;
    batch(() => {
      setQuoteTarget(target);
      setQuoteOccurrence(index);
      setRound(target.round);
      setMessage(location.kind === 'found' ? location.occurrences[index]!.firstMessage : 0);
    });
  };
  const setActiveQuoteOccurrence = (index: number) => {
    const occurrence = quoteOccurrences()[index];
    if (!occurrence) return;
    batch(() => {
      setQuoteOccurrence(index);
      setMessage(occurrence.firstMessage);
    });
  };
  const clearQuote = () =>
    batch(() => {
      setQuoteTarget(undefined);
      setQuoteOccurrence(0);
    });

  const creatingCall = createMemo(() => {
    const trial = activeTrial();
    return trial
      ? resolveCreatingCall(trial, options.stages(), options.personIndex())
      : { kind: 'unavailable' as const };
  });

  const workspacePosition = () => highlight() ?? positionFor(positionKey());

  return {
    rounds,
    unpublishedRounds,
    activeRound,
    selectRound,
    items,
    activeMessageIndex,
    current,
    selectMessage,
    stepMessage,
    quoteTarget,
    quoteLocation,
    quoteOccurrences,
    quoteOccurrenceIndex,
    revealQuote,
    setQuoteOccurrence: setActiveQuoteOccurrence,
    clearQuote,
    highlight,
    workspacePosition,
    positionKey,
    positionFor,
    savePosition,
    creatingCall,
  };
}

export type ReplyNavigation = ReturnType<typeof createReplyNavigation>;
