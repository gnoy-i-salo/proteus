import type * as SolidClient from 'solid-js';
import { createRoot, createSignal } from 'solid-js';
import { expect, it, vi } from 'vitest';
import type { IntelligenceResult } from '@proteus-int/contracts';
import type { ExperimentPerson, StageTrace, TrialResult } from '@proteus-int/pipeline';
import {
  createReplyNavigation,
  locateQuote,
  occurrenceOverlap,
  replyMessagesForTrial,
  resolveCreatingCall,
} from './reply-navigation';

vi.mock('solid-js', () => vi.importActual<typeof SolidClient>('solid-js/dist/solid.js'));

function traceFor(provider: 'codex' | 'kimi'): IntelligenceResult['trace'] {
  return { provider, reasoningMs: 0, terminalMs: 0, summary: '' };
}
function attempt(partial: Partial<IntelligenceResult> & { answer: string }): IntelligenceResult {
  return { trace: traceFor('codex'), ...partial };
}
function trial(partial: Partial<TrialResult> & { round: number }): TrialResult {
  return {
    id: `trial-${partial.round}`,
    personId: 'person-a',
    status: 'evaluated',
    attempt: null,
    score: null,
    evaluations: [],
    error: null,
    ...partial,
  };
}
function person(partial: Partial<ExperimentPerson>): ExperimentPerson {
  return {
    report: null,
    baseProfile: undefined as never,
    profile: { id: 'person-a', label: 'A' } as never,
    attempt: null,
    score: undefined as never,
    trials: [],
    preparationWallTimeMs: 0,
    solutionWallTimeMs: 0,
    baselineDifference: null,
    artifacts: {},
    ...partial,
  };
}
function stageWithCalls(
  id: string,
  personIndex: number | null,
  calls: readonly {
    callId: string;
    trigger: 'initial' | 'formatRepair';
    status?: 'completed' | 'failed';
  }[],
): StageTrace {
  return {
    id,
    stage: 'person',
    person: personIndex,
    label: `Exact ${id}`,
    attempts: [],
    trace: traceFor('kimi'),
    lifecycle: {
      schemaVersion: 1,
      runId: 'run-1',
      stageId: `stage-${id}`,
      logicalStageId: id,
      component: 'person',
      person: personIndex,
      label: `Exact ${id}`,
      status: 'completed',
      startedAt: null,
      finishedAt: null,
      error: null,
      calls: calls.map((call) => ({
        callId: call.callId,
        operation: 'persona-reply',
        trigger: call.trigger,
        formatAttempt: call.trigger === 'formatRepair' ? 2 : 1,
        status: call.status ?? 'completed',
        startedAt: '2026-11-08T00:00:00Z',
        returnedAt: null,
        finishedAt: null,
        wallTimeMs: 100,
        error: null,
        validationError: null,
      })),
    },
  };
}

it('keeps published messages separate and marks failed or missing trials as unpublished', () => {
  const answered = trial({
    round: 1,
    status: 'answered',
    attempt: attempt({
      answer: 'один\n\nдва',
      reply: { version: 2, messages: ['один', 'два'], styleConflicts: [] },
    }),
  });
  expect(replyMessagesForTrial(answered)).toEqual([
    { index: 0, number: 1, total: 2, text: 'один', published: true },
    { index: 1, number: 2, total: 2, text: 'два', published: true },
  ]);
  const answerOnly = trial({ round: 2, attempt: attempt({ answer: 'только ответ' }) });
  expect(replyMessagesForTrial(answerOnly)).toEqual([
    { index: 0, number: 1, total: 1, text: 'только ответ', published: true },
  ]);
  const failed = trial({ round: 3, status: 'failed', error: 'CLI недоступен' });
  expect(replyMessagesForTrial(failed)).toEqual([
    { index: 0, number: 1, total: 1, text: 'CLI недоступен', published: false },
  ]);
  const pending = trial({ round: 4, status: 'pending' });
  expect(replyMessagesForTrial(pending)).toEqual([
    { index: 0, number: 1, total: 1, text: 'Ответ не получен.', published: false },
  ]);
});

it('locates quotes by exact joined offsets, including repeats and spans across messages', () => {
  const joined = 'alfa  bravo\n\nalfa 🚀\r\nline';
  const messages = ['alfa  bravo', 'alfa 🚀\r\nline'];
  const single = locateQuote(attempt({ answer: joined, reply: reply(messages) }), 'bravo');
  expect(single).toEqual({
    kind: 'found',
    occurrences: [{ start: 6, end: 11, firstMessage: 0, lastMessage: 0 }],
  });
  const repeated = locateQuote(attempt({ answer: joined, reply: reply(messages) }), 'alfa');
  expect(repeated.kind).toBe('found');
  if (repeated.kind !== 'found') return;
  expect(repeated.occurrences).toEqual([
    { start: 0, end: 4, firstMessage: 0, lastMessage: 0 },
    { start: 13, end: 17, firstMessage: 1, lastMessage: 1 },
  ]);
  const spanning = locateQuote(
    attempt({ answer: joined, reply: reply(messages) }),
    'bravo\n\nalfa',
  );
  expect(spanning).toEqual({
    kind: 'found',
    occurrences: [{ start: 6, end: 17, firstMessage: 0, lastMessage: 1 }],
  });
  const crlfExact = locateQuote(attempt({ answer: joined, reply: reply(messages) }), '\r\nline');
  expect(crlfExact.kind).toBe('found');
  const caseSensitive = locateQuote(attempt({ answer: joined, reply: reply(messages) }), 'ALFA');
  expect(caseSensitive).toEqual({ kind: 'absent' });
});

it('reports absent and unavailable quote locations without guessing', () => {
  const answered = attempt({ answer: 'текст' });
  expect(locateQuote(answered, '')).toEqual({ kind: 'absent' });
  expect(locateQuote(answered, 'нет в ответе')).toEqual({ kind: 'absent' });
  expect(locateQuote(null, 'текст')).toEqual({ kind: 'unavailable' });
  expect(locateQuote(attempt({ answer: '' }), 'текст')).toEqual({ kind: 'unavailable' });
});

it('maps occurrence overlaps onto exact per-message ranges', () => {
  const lengths = [11, 13];
  const middle = { start: 10, end: 18, firstMessage: 0, lastMessage: 1 };
  expect(occurrenceOverlap(middle, 0, lengths)).toEqual({ start: 10, end: 11 });
  expect(occurrenceOverlap(middle, 1, lengths)).toEqual({ start: 0, end: 5 });
  expect(
    occurrenceOverlap({ start: 6, end: 17, firstMessage: 0, lastMessage: 1 }, 1, lengths),
  ).toEqual({ start: 0, end: 4 });
  expect(
    occurrenceOverlap({ start: 0, end: 4, firstMessage: 0, lastMessage: 0 }, 1, lengths),
  ).toBeUndefined();
});

it('links a published answer to its creating call only through stable call metadata', () => {
  const stages = [
    stageWithCalls('solve-1-a', 0, [{ callId: 'call-1', trigger: 'initial' }]),
    stageWithCalls('solve-1-b', 1, [
      { callId: 'call-2', trigger: 'initial' },
      { callId: 'call-3', trigger: 'formatRepair' },
    ]),
  ];
  const linked = resolveCreatingCall(
    trial({ round: 1, attempt: attempt({ answer: 'x', invocationId: 'call-3' }) }),
    stages,
    null,
  );
  expect(linked.kind).toBe('linked');
  if (linked.kind !== 'linked') return;
  expect(linked.stage.id).toBe('solve-1-b');
  expect(linked.call.trigger).toBe('formatRepair');
  expect(
    resolveCreatingCall(trial({ round: 1, attempt: attempt({ answer: 'x' }) }), stages, null).kind,
  ).toBe('unavailable');
  expect(
    resolveCreatingCall(
      trial({ round: 1, attempt: attempt({ answer: 'x', invocationId: 'unknown-call' }) }),
      stages,
      null,
    ).kind,
  ).toBe('unavailable');
  const legacyStage: StageTrace = {
    id: 'solve-1-a',
    stage: 'person',
    person: 0,
    label: 'Legacy',
    attempts: [],
    trace: traceFor('codex'),
  };
  expect(
    resolveCreatingCall(
      trial({ round: 1, attempt: attempt({ answer: 'x', invocationId: 'call-1' }) }),
      [legacyStage],
      null,
    ).kind,
  ).toBe('unavailable');
});

it('links through the public trace classification only when the call is unique', () => {
  const repair = stageWithCalls('solve-1-b', 1, [
    { callId: 'call-2', trigger: 'initial', status: 'failed' },
    { callId: 'call-3', trigger: 'formatRepair' },
  ]);
  const repaired = resolveCreatingCall(
    trial({ round: 1, attempt: attempt({ answer: 'x' }) }),
    [repair],
    1,
  );
  expect(repaired).toMatchObject({
    kind: 'linked',
    stage: { id: 'solve-1-b' },
    call: { callId: 'call-3', trigger: 'formatRepair' },
  });
  expect(
    resolveCreatingCall(trial({ round: 1, attempt: attempt({ answer: 'x' }) }), [], 1).kind,
  ).toBe('unavailable');
  expect(
    resolveCreatingCall(trial({ round: 2, attempt: attempt({ answer: 'x' }) }), [repair], 1).kind,
  ).toBe('unavailable');
  expect(
    resolveCreatingCall(trial({ round: 1, attempt: attempt({ answer: 'x' }) }), [repair], null)
      .kind,
  ).toBe('unavailable');
  const twoCompleted = stageWithCalls('solve-1-a', 0, [
    { callId: 'call-a', trigger: 'initial' },
    { callId: 'call-b', trigger: 'initial' },
  ]);
  expect(
    resolveCreatingCall(trial({ round: 1, attempt: attempt({ answer: 'x' }) }), [twoCompleted], 0)
      .kind,
  ).toBe('unavailable');
  const legacy: StageTrace = {
    id: 'solve-1-a',
    stage: 'person',
    person: 0,
    label: 'Legacy',
    attempts: [],
    trace: traceFor('codex'),
  };
  expect(
    resolveCreatingCall(trial({ round: 1, attempt: attempt({ answer: 'x' }) }), [legacy], 0).kind,
  ).toBe('unavailable');
});

function reply(messages: readonly string[]) {
  return { version: 2 as const, messages, styleConflicts: [] };
}

function setupModel(
  trials: readonly TrialResult[],
  stages: readonly StageTrace[] = [],
  personIndex: number | null = 0,
) {
  return createRoot((dispose) => {
    const [current, setPerson] = createSignal<ExperimentPerson | undefined>(
      person({ trials, profile: { id: 'person-a', label: 'A' } as never }),
    );
    const model = createReplyNavigation({
      person: current,
      personIndex: () => personIndex,
      stages: () => stages,
    });
    return { model, setPerson, dispose };
  });
}

it('selects rounds and messages individually and clamps when the round changes', () => {
  const s = setupModel([
    trial({
      round: 1,
      attempt: attempt({ answer: 'один\n\nдва', reply: reply(['один', 'два']) }),
    }),
    trial({ round: 2, attempt: attempt({ answer: 'три', reply: reply(['три']) }) }),
  ]);
  expect(s.model.rounds()).toEqual([1, 2]);
  expect(s.model.activeRound()).toBe(1);
  expect(s.model.activeMessageIndex()).toBe(0);
  expect(s.model.current()?.text).toBe('один');
  s.model.selectMessage(1);
  expect(s.model.current()?.text).toBe('два');
  s.model.selectRound(2);
  expect(s.model.activeMessageIndex()).toBe(0);
  expect(s.model.current()?.text).toBe('три');
  s.model.selectMessage(9);
  expect(s.model.activeMessageIndex()).toBe(0);
  s.dispose();
});

it('resets selection and quote target when the participant changes', () => {
  const s = setupModel([
    trial({ round: 1, attempt: attempt({ answer: 'alfa bravo', reply: reply(['alfa bravo']) }) }),
  ]);
  s.model.selectMessage(0);
  s.model.revealQuote({ quote: 'bravo', round: 1 });
  expect(s.model.quoteTarget()?.quote).toBe('bravo');
  s.setPerson(person({ trials: [], profile: { id: 'person-b', label: 'B' } as never }));
  expect(s.model.activeRound()).toBeNull();
  expect(s.model.quoteTarget()).toBeUndefined();
  s.dispose();
});

it('reveals the exact occurrence and moves between repeated occurrences explicitly', () => {
  const s = setupModel([
    trial({
      round: 1,
      attempt: attempt({
        answer: 'alfa beta alfa',
        reply: reply(['alfa beta alfa']),
      }),
    }),
  ]);
  s.model.revealQuote({ quote: 'alfa', round: 1 });
  expect(s.model.quoteOccurrences()).toHaveLength(2);
  expect(s.model.quoteOccurrenceIndex()).toBe(0);
  const first = s.model.highlight();
  expect(first).toMatchObject({ anchor: 0, query: 'alfa', match: 0 });
  s.model.setQuoteOccurrence(1);
  expect(s.model.quoteOccurrenceIndex()).toBe(1);
  expect(s.model.highlight()).toMatchObject({ anchor: 10, query: 'alfa', match: 1 });
  s.model.revealQuote({ quote: 'alfa', round: 1, occurrence: 1 });
  expect(s.model.quoteOccurrenceIndex()).toBe(1);
  s.model.clearQuote();
  expect(s.model.quoteTarget()).toBeUndefined();
  expect(s.model.highlight()).toBeUndefined();
  s.dispose();
});

it('navigates to the message containing a spanning quote and keeps other messages unhighlighted', () => {
  const s = setupModel([
    trial({
      round: 1,
      attempt: attempt({ answer: 'один\n\nдва', reply: reply(['один', 'два']) }),
    }),
  ]);
  s.model.revealQuote({ quote: 'один\n\nдва', round: 1 });
  expect(s.model.activeMessageIndex()).toBe(0);
  expect(s.model.highlight()).toMatchObject({ anchor: 0, query: 'один' });
  s.model.selectMessage(1);
  expect(s.model.highlight()).toMatchObject({ anchor: 0, query: 'два' });
  s.model.selectMessage(0);
  expect(s.model.highlight()?.query).toBe('один');
  s.dispose();
});

it('keeps unreadable quotes explicit and never fabricates a highlight', () => {
  const s = setupModel([
    trial({ round: 1, attempt: attempt({ answer: 'текст', reply: reply(['текст']) }) }),
    trial({ round: 2, status: 'failed', error: 'сбой этапа' }),
  ]);
  s.model.revealQuote({ quote: 'нет в ответе', round: 1 });
  expect(s.model.quoteLocation()?.kind).toBe('absent');
  expect(s.model.highlight()).toBeUndefined();
  s.model.revealQuote({ quote: 'текст', round: 2 });
  expect(s.model.quoteLocation()?.kind).toBe('unavailable');
  expect(s.model.highlight()).toBeUndefined();
  s.dispose();
});

it('saves reader positions per message and protects them from quote highlights', () => {
  const s = setupModel([
    trial({
      round: 1,
      attempt: attempt({ answer: 'один\n\nдва', reply: reply(['один', 'два']) }),
    }),
  ]);
  const position = {
    anchor: 2,
    selection: { start: 0, end: 2, direction: 'forward' as const },
    query: '',
    match: -1,
  };
  const key = s.model.positionKey();
  expect(key).toBe('person-a:1:0');
  s.model.savePosition(key, position);
  expect(s.model.positionFor(key)).toEqual(position);
  s.model.selectMessage(1);
  expect(s.model.positionKey()).toBe('person-a:1:1');
  expect(s.model.positionFor('person-a:1:1')).toBeUndefined();
  s.model.selectMessage(0);
  expect(s.model.positionFor(key)).toEqual(position);
  s.model.revealQuote({ quote: 'один', round: 1 });
  s.model.savePosition(key, { ...position, anchor: 0 });
  expect(s.model.positionFor(key)?.anchor).toBe(2);
  s.dispose();
});

it('resolves the creating call for the active round only from stable metadata', () => {
  const stages = [
    stageWithCalls('solve-1-a', 0, [{ callId: 'call-1', trigger: 'initial' }]),
    stageWithCalls('solve-2-a', 0, [{ callId: 'call-2', trigger: 'initial' }]),
  ];
  const s = setupModel(
    [
      trial({
        round: 1,
        attempt: attempt({ answer: 'a1', invocationId: 'call-1', reply: reply(['a1']) }),
      }),
      trial({
        round: 2,
        attempt: attempt({ answer: 'a2', invocationId: 'call-2', reply: reply(['a2']) }),
      }),
    ],
    stages,
    0,
  );
  expect(s.model.creatingCall()).toMatchObject({ kind: 'linked', call: { callId: 'call-1' } });
  s.model.selectRound(2);
  expect(s.model.creatingCall()).toMatchObject({ kind: 'linked', call: { callId: 'call-2' } });
  s.setPerson(person({ trials: [], profile: { id: 'person-b', label: 'B' } as never }));
  expect(s.model.creatingCall().kind).toBe('unavailable');
  s.dispose();
});

it('does not map quote offsets through inconsistent joined-answer metadata', () => {
  expect(
    locateQuote(
      attempt({ answer: 'original answer', reply: reply(['different message']) }),
      'answer',
    ),
  ).toEqual({ kind: 'unavailable' });
});

it('never assigns a creating call to an absent answer, failed call or duplicate call ID', () => {
  const completed = stageWithCalls('solve-1-a', 0, [{ callId: 'accepted', trigger: 'initial' }]);
  expect(resolveCreatingCall(trial({ round: 1 }), [completed], 0).kind).toBe('unavailable');
  const failed = stageWithCalls('solve-1-a', 0, [
    { callId: 'failed', trigger: 'initial', status: 'failed' },
  ]);
  expect(
    resolveCreatingCall(
      trial({ round: 1, attempt: attempt({ answer: 'x', invocationId: 'failed' }) }),
      [failed],
      0,
    ).kind,
  ).toBe('unavailable');
  const duplicate = stageWithCalls('solve-1-b', 1, [{ callId: 'accepted', trigger: 'initial' }]);
  expect(
    resolveCreatingCall(
      trial({ round: 1, attempt: attempt({ answer: 'x', invocationId: 'accepted' }) }),
      [completed, duplicate],
      0,
    ).kind,
  ).toBe('unavailable');
});

it('resets quote and reader position when the same saved persona changes A/B roles', () => {
  const trials = [trial({ round: 1, attempt: attempt({ answer: 'alfa bravo' }) })];
  const s = setupModel(trials);
  s.setPerson(person({ participantId: 'a', trials }));
  s.model.revealQuote({ quote: 'bravo', round: 1 });
  expect(s.model.quoteTarget()?.quote).toBe('bravo');
  const key = s.model.positionKey();
  s.setPerson(person({ participantId: 'b', trials }));
  expect(s.model.quoteTarget()).toBeUndefined();
  expect(s.model.positionKey()).not.toBe(key);
  s.dispose();
});
