import { batch, createEffect, createMemo, createSignal } from 'solid-js';
import type { StageTrace } from '@proteus-int/pipeline';
import type { TextWorkspacePosition } from '../../../shared/ui/text-workspace/text-workspace-model';
import { classifyTraceStage } from './execution-stage';

export type TraceParticipantFilter = 'all' | 'a' | 'b' | 'control' | 'system';
export type TracePhaseFilter = 'all' | 'preparation' | 'trial';
export type TraceReaderPosition = TextWorkspacePosition;
type ReaderKind = 'reasoning' | 'summary';

export function createTraceNavigation(options: {
  stages: () => readonly StageTrace[];
  running: () => boolean;
  activeStageId: () => string | undefined;
  failedStageId: () => string | undefined;
}) {
  const [participant, changeParticipant] = createSignal<TraceParticipantFilter>('all');
  const [phase, changePhase] = createSignal<TracePhaseFilter>('all');
  const [round, changeRound] = createSignal<number | null>(null);
  const [chosen, changeChosen] = createSignal('');
  const [following, setFollowing] = createSignal(true);
  const [manual, setManual] = createSignal(false);
  const choices = new Map<string, string>();
  const positions = new Map<string, TraceReaderPosition>();
  const classified = createMemo(() =>
    options.stages().map((stage) => ({ stage, identity: classifyTraceStage(stage) })),
  );
  const filterKey = () => `${participant()}/${phase()}/${round() ?? 'all'}`;
  const stages = createMemo(() =>
    classified()
      .filter(({ identity }) => {
        if (participant() !== 'all' && identity.participant !== participant()) return false;
        if (round() !== null && identity.round !== round()) return false;
        if (phase() === 'trial') return identity.round !== null;
        if (phase() === 'preparation')
          return identity.round === null && identity.phase !== 'unknown';
        return true;
      })
      .map(({ stage }) => stage),
  );
  const availableRounds = createMemo(() => {
    const rounds = new Set<number>();
    for (const { identity } of classified())
      if (identity.round !== null) rounds.add(identity.round);
    return [...rounds].sort((first, second) => first - second);
  });
  const active = () => stages().find((stage) => stage.id === chosen()) ?? stages()[0];
  const current = () =>
    options.stages().find((stage) => stage.id === options.activeStageId()) ??
    options.stages().find((stage) => stage.id === options.failedStageId()) ??
    options.stages().at(-1);
  const allFilters = () => {
    changeParticipant('all');
    changePhase('all');
    changeRound(null);
  };
  const rememberChoice = () => {
    const id = active()?.id;
    if (id) choices.set(filterKey(), id);
  };
  const pause = () =>
    batch(() => {
      setManual(true);
      setFollowing(false);
      const id = active()?.id;
      if (id) changeChosen(id);
    });
  const filter = (change: () => void) => {
    rememberChoice();
    batch(() => {
      setManual(true);
      setFollowing(false);
      change();
      changeChosen(choices.get(filterKey()) ?? '');
    });
  };
  const reveal = (id: string) =>
    batch(() => {
      if (!stages().some((stage) => stage.id === id)) allFilters();
      changeChosen(id);
      choices.set(filterKey(), id);
    });
  const select = (id: string) => {
    if (!options.stages().some((stage) => stage.id === id)) return;
    setManual(true);
    setFollowing(false);
    reveal(id);
  };
  const resume = () =>
    batch(() => {
      setManual(false);
      allFilters();
      changeChosen(current()?.id ?? '');
      setFollowing(true);
    });
  const reset = () =>
    batch(() => {
      choices.clear();
      positions.clear();
      setManual(false);
      allFilters();
      changeChosen('');
      setFollowing(true);
    });
  createEffect(() => {
    if (options.running() && following()) changeChosen(current()?.id ?? '');
  });
  createEffect(() => {
    const failedId = options.failedStageId();
    if (!failedId || options.running() || manual()) return;
    reveal(failedId);
  });
  return {
    participant,
    setParticipant: (value: TraceParticipantFilter) => filter(() => changeParticipant(value)),
    phase,
    setPhase: (value: TracePhaseFilter) =>
      filter(() => {
        changePhase(value);
        changeRound(null);
      }),
    round,
    setRound: (value: number | null) => filter(() => changeRound(value)),
    availableRounds,
    hasStages: () => options.stages().length > 0,
    stages,
    active,
    following,
    manual,
    select,
    pause,
    resume,
    reset,
    positionFor: (id: string | undefined, reader: ReaderKind = 'reasoning') =>
      id ? positions.get(JSON.stringify([reader, id])) : undefined,
    savePosition: (id: string, position: TraceReaderPosition, reader: ReaderKind = 'reasoning') => {
      positions.set(JSON.stringify([reader, id]), position);
    },
  };
}
export type TraceNavigation = ReturnType<typeof createTraceNavigation>;
