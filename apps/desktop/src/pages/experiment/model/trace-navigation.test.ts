import type * as SolidClient from 'solid-js';
import { createRoot, createSignal } from 'solid-js';
import { expect, it, vi } from 'vitest';
import type { StageTrace } from '@proteus-int/pipeline';
import { createTraceNavigation } from './trace-navigation';
vi.mock('solid-js', () => vi.importActual<typeof SolidClient>('solid-js/dist/solid.js'));
function stage(id: string, component: StageTrace['stage'], person: number | null): StageTrace {
  return {
    id,
    stage: component,
    person,
    label: `Exact ${id}`,
    attempts: [],
    trace: { provider: 'codex', reasoningMs: 0, terminalMs: 0, summary: '' },
  };
}
const initial = [
  stage('rubric', 'brain-analyzator', null),
  stage('sources-0', 'stalker', 0),
  stage('profile-1', 'person-fabric', 1),
  stage('adapt-2', 'person-fabric', 2),
  stage('solve-1-a', 'person', 0),
  stage('solve-2-b', 'person', 1),
  stage('solve-3-neutral', 'person', 2),
  stage('judge-3-1', 'brain-analyzator', null),
];
function setup(runningInitially = true) {
  return createRoot((dispose) => {
    const [stages, setStages] = createSignal<readonly StageTrace[]>(initial);
    const [running, setRunning] = createSignal(runningInitially);
    const [activeStageId, setActiveStageId] = createSignal<string | undefined>('solve-2-b');
    const [failedStageId, setFailedStageId] = createSignal<string | undefined>(undefined);
    const model = createTraceNavigation({ stages, running, activeStageId, failedStageId });
    return { model, setStages, setRunning, setActiveStageId, setFailedStageId, dispose };
  });
}
it('follows the declared active stage, then preserves manual selection during streaming and completion', () => {
  const s = setup();
  expect(s.model.active()?.id).toBe('solve-2-b');
  expect(s.model.following()).toBe(true);
  s.model.select('sources-0');
  expect(s.model.following()).toBe(false);
  expect(s.model.manual()).toBe(true);
  s.setActiveStageId('judge-3-1');
  s.setStages([...initial, stage('legacy-exact', 'person', 0)]);
  expect(s.model.active()?.id).toBe('sources-0');
  s.setRunning(false);
  expect(s.model.active()?.id).toBe('sources-0');
  s.dispose();
});
it('filters all participants, preparation and trial rounds without deriving legacy metadata from a label', () => {
  const s = setup(false);
  s.model.setParticipant('control');
  expect(s.model.stages().map((item) => item.id)).toEqual(['adapt-2', 'solve-3-neutral']);
  s.model.setPhase('preparation');
  expect(s.model.stages().map((item) => item.id)).toEqual(['adapt-2']);
  s.model.setParticipant('all');
  s.model.setPhase('trial');
  s.model.setRound(3);
  expect(s.model.stages().map((item) => item.id)).toEqual(['solve-3-neutral', 'judge-3-1']);
  s.model.setParticipant('system');
  expect(s.model.stages().map((item) => item.id)).toEqual(['judge-3-1']);
  s.model.setPhase('all');
  s.model.setRound(null);
  expect(s.model.stages().map((item) => item.id)).toEqual(['rubric', 'judge-3-1']);
  s.model.setParticipant('all');
  s.setStages([...initial, stage('legacy-exact', 'person', 0)]);
  expect(s.model.stages().map((item) => item.id)).toContain('legacy-exact');
  s.model.setPhase('preparation');
  expect(s.model.stages().map((item) => item.id)).not.toContain('legacy-exact');
  s.model.setPhase('trial');
  expect(s.model.stages().map((item) => item.id)).not.toContain('legacy-exact');
  expect(s.model.following()).toBe(false);
  s.dispose();
});
it('returns to the chosen stage per filter and resumes current stage in one command', () => {
  const s = setup();
  s.model.setParticipant('a');
  s.model.select('solve-1-a');
  s.model.setParticipant('b');
  s.model.select('profile-1');
  s.model.setParticipant('a');
  expect(s.model.active()?.id).toBe('solve-1-a');
  expect(s.model.manual()).toBe(true);
  s.model.resume();
  expect(s.model.following()).toBe(true);
  expect(s.model.manual()).toBe(false);
  expect(s.model.participant()).toBe('all');
  expect(s.model.phase()).toBe('all');
  expect(s.model.round()).toBeNull();
  expect(s.model.active()?.id).toBe('solve-2-b');
  s.dispose();
});
it('opens a requested failure regardless of current filters and does not change selection for a missing id', () => {
  const s = setup();
  s.model.setParticipant('a');
  s.model.select('judge-3-1');
  expect(s.model.active()?.id).toBe('judge-3-1');
  expect(s.model.participant()).toBe('all');
  s.model.select('missing');
  expect(s.model.active()?.id).toBe('judge-3-1');
  s.dispose();
});
it('selects the failed stage initially after a terminal failure without an intentional manual selection', () => {
  const s = setup();
  s.setActiveStageId(undefined);
  s.setFailedStageId('judge-3-1');
  s.setRunning(false);
  expect(s.model.active()?.id).toBe('judge-3-1');
  expect(s.model.participant()).toBe('all');
  s.dispose();
});
it('never overrides an intentional manual reading selection when a failure arrives', () => {
  const s = setup();
  s.model.select('sources-0');
  s.setActiveStageId(undefined);
  s.setFailedStageId('judge-3-1');
  s.setRunning(false);
  expect(s.model.active()?.id).toBe('sources-0');
  s.model.resume();
  expect(s.model.active()?.id).toBe('judge-3-1');
  s.dispose();
});
it('keeps a reading position per stage id across switches and clears it only for an explicit new run', () => {
  const s = setup(false);
  const position = {
    anchor: 42,
    selection: { start: 40, end: 42, direction: 'forward' as const },
    query: 'искомый текст',
    match: 1,
    bookmark: { anchor: 5, selection: { start: 5, end: 5, direction: 'none' as const } },
  };
  s.model.savePosition('solve-1-a', position);
  expect(s.model.positionFor('solve-1-a')).toEqual(position);
  expect(s.model.positionFor('solve-2-b')).toBeUndefined();
  expect(s.model.positionFor(undefined)).toBeUndefined();
  s.model.select('profile-1');
  s.model.setParticipant('b');
  expect(s.model.positionFor('solve-1-a')).toEqual(position);
  s.model.reset();
  expect(s.model.positionFor('solve-1-a')).toBeUndefined();
  expect(s.model.participant()).toBe('all');
  expect(s.model.following()).toBe(true);
  s.dispose();
});
it('does not follow historical details and resets navigation only for an explicit new run', () => {
  const s = setup(false);
  expect(s.model.active()?.id).toBe('rubric');
  s.model.select('profile-1');
  s.setStages([...initial, stage('legacy', 'person', 1)]);
  expect(s.model.active()?.id).toBe('profile-1');
  s.model.setParticipant('b');
  s.model.reset();
  expect(s.model.participant()).toBe('all');
  expect(s.model.following()).toBe(true);
  expect(s.model.active()?.id).toBe('rubric');
  s.dispose();
});
it('exposes the trial rounds present in the classifier output', () => {
  const s = setup(false);
  expect(s.model.availableRounds()).toEqual([1, 2, 3]);
  s.setStages(initial.filter((item) => !item.id.endsWith('-3-neutral') && item.id !== 'judge-3-1'));
  expect(s.model.availableRounds()).toEqual([1, 2]);
  s.dispose();
});

it('keeps summary and reasoning positions separate for the same stage', () => {
  const s = setup(false);
  const position = (anchor: number) => ({
    anchor,
    selection: { start: anchor, end: anchor, direction: 'none' as const },
    query: '',
    match: -1,
  });
  s.model.savePosition('rubric', position(12));
  s.model.savePosition('rubric', position(45), 'summary');
  expect(s.model.positionFor('rubric')).toEqual(position(12));
  expect(s.model.positionFor('rubric', 'summary')).toEqual(position(45));
  s.model.reset();
  expect(s.model.positionFor('rubric', 'summary')).toBeUndefined();
  s.dispose();
});

it('remembers a content view for each stage without resuming live follow', () => {
  const s = setup(false);
  s.model.select('rubric');
  s.model.setContentView('artifact');
  expect(s.model.contentView()).toBe('artifact');
  expect(s.model.following()).toBe(false);
  s.model.select('solve-1-a');
  expect(s.model.contentView()).toBe('reasoning');
  s.model.setContentView('published');
  s.model.select('rubric');
  expect(s.model.contentView()).toBe('artifact');
  s.model.reset();
  expect(s.model.contentView()).toBe('reasoning');
  s.dispose();
});

it('changing a live content view pauses follow and retains separate reader positions', () => {
  const s = setup(true);
  s.model.setContentView('calls');
  expect(s.model.following()).toBe(false);
  const position = {
    anchor: 23,
    selection: { start: 23, end: 23, direction: 'none' as const },
    query: '',
    match: -1,
  };
  s.model.savePosition('rubric', position, 'artifact');
  expect(s.model.positionFor('rubric', 'artifact')).toEqual(position);
  expect(s.model.positionFor('rubric', 'published')).toBeUndefined();
  expect(s.model.positionFor('rubric', 'calls')).toBeUndefined();
  s.model.reset();
  expect(s.model.positionFor('rubric', 'artifact')).toBeUndefined();
  s.dispose();
});
