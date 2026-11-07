import type {
  AnswerEvaluation,
  ExperimentInput,
  ExperimentProtocol,
  IntelligenceResult,
  IntelligenceTrace,
  InvocationAttempt,
  PersonProfile,
  ProviderId,
  ScoreInterval,
  ScoreSummary,
  TrialScore,
  SourceGrant,
  SourcePort,
} from '@proteus-int/contracts';
import type { ComponentId, IntelligenceProvider, ModelSettings } from '@proteus-int/intelligence';
import { componentIds, IntelligenceRunError } from '@proteus-int/intelligence';
import {
  analyzeSourceCollections,
  collectPerson,
  collectSource,
  validateSourceGrant,
} from '@proteus-int/stalker';
import type { SourceCollection, StalkerReport } from '@proteus-int/stalker';
import { adaptPerson, fabricatePerson, neutralProfile } from '@proteus-int/person-fabric';
import { personaArtifacts, personRuntime } from '@proteus-int/person';
import { difference, scoreTrial, summarizeScores } from '@proteus-int/brain-scoring';
import { createRubric, evaluateAnswers } from '@proteus-int/brain-analyzator';
import { validateExperiment } from './validation';

export interface TrialResult {
  readonly id: string;
  readonly round: number;
  readonly 'person-id': string;
  readonly status: 'pending' | 'answered' | 'evaluated' | 'failed' | 'skipped';
  readonly attempt: IntelligenceResult | null;
  readonly score: TrialScore | null;
  readonly evaluations: readonly AnswerEvaluation[];
  readonly error: string | null;
}
export interface ExperimentPerson {
  readonly report: StalkerReport | null;
  readonly 'base-profile': PersonProfile;
  readonly profile: PersonProfile;
  readonly attempt: IntelligenceResult | null;
  readonly score: ScoreSummary;
  readonly trials: readonly TrialResult[];
  readonly 'preparation-wall-time-ms': number;
  readonly 'solution-wall-time-ms': number;
  readonly 'baseline-difference': ScoreInterval | null;
  readonly artifacts: Readonly<Record<string, string>>;
}
export interface StageTrace {
  readonly id: string;
  readonly stage: ComponentId;
  readonly person: number | null;
  readonly label: string;
  readonly trace: IntelligenceTrace;
  readonly attempts: readonly InvocationAttempt[];
}
export interface ExperimentResult {
  readonly 'schema-version': 3;
  readonly 'scoring-version': 2;
  readonly status: 'complete' | 'incomplete' | 'cancelled';
  readonly input: ExperimentInput;
  readonly protocol: ExperimentProtocol;
  readonly people: readonly ExperimentPerson[];
  readonly control: ExperimentPerson;
  readonly trials: readonly TrialResult[];
  readonly comparison: {
    readonly summary: string;
    readonly difference: ScoreInterval | null;
    readonly 'comparison-eligible': boolean;
  };
  readonly 'finished-at': string;
  readonly traces: readonly StageTrace[];
}
export interface ProgressEvent {
  readonly id: string;
  readonly stage: ComponentId | 'complete';
  readonly person: number | null;
  readonly completed: number;
  readonly total: number;
  readonly message: string;
  readonly trace?: IntelligenceTrace;
}
export interface RunOptions {
  readonly sourcePort?: SourcePort;
  readonly sourceGrants?: readonly [readonly SourceGrant[], readonly SourceGrant[]];
  readonly repetitions?: 1 | 3;
  readonly signal?: AbortSignal;
  readonly onProtocol?: (protocol: ExperimentProtocol) => Promise<void>;
  readonly onProgress?: (event: ProgressEvent) => void;
  readonly onPerson?: (person: ExperimentPerson) => void;
  readonly onTrace?: (trace: StageTrace) => void;
}

export function buildTrialOrder(repetitions: 1 | 3): readonly (readonly string[])[] {
  if (repetitions !== 1 && repetitions !== 3) throw new Error('Нужен 1 или 3 прогона.');
  const ids = ['a', 'b', 'neutral'];
  return Array.from({ length: repetitions }, (_, round) =>
    ids.map((_, offset) => ids[(round + offset) % ids.length]!),
  );
}
function freeze<T>(value: T): T {
  if (value && typeof value === 'object') {
    Object.values(value).forEach(freeze);
    Object.freeze(value);
  }
  return value;
}
function duration(trace: IntelligenceTrace) {
  return trace['wall-time-ms'] ?? trace['reasoning-ms'];
}
async function reviewProtocol(protocol: ExperimentProtocol, options: RunOptions) {
  options.signal?.throwIfAborted();
  if (!options.onProtocol) return;
  await new Promise<void>((resolve, reject) => {
    const abort = () => reject(options.signal?.reason ?? new Error('Отменено'));
    options.signal?.addEventListener('abort', abort, { once: true });
    Promise.resolve()
      .then(() => options.onProtocol!(protocol))
      .then(resolve, reject)
      .finally(() => options.signal?.removeEventListener('abort', abort));
  });
  options.signal?.throwIfAborted();
}

export async function runExperiment(
  sourceInput: ExperimentInput,
  sourceSettings: ModelSettings,
  providers: Readonly<Record<ProviderId, IntelligenceProvider>>,
  options: RunOptions = {},
): Promise<ExperimentResult> {
  const input = freeze(structuredClone(sourceInput));
  const settings = freeze(structuredClone(sourceSettings));
  const sourceGrants = freeze(structuredClone(options.sourceGrants ?? [[], []])) as readonly [
    readonly SourceGrant[],
    readonly SourceGrant[],
  ];
  const sourcePort = options.sourcePort;
  if (
    !Array.isArray(sourceGrants) ||
    sourceGrants.length !== 2 ||
    sourceGrants.some((grants) => !Array.isArray(grants) || grants.length > 5)
  )
    throw new Error('Выберите до пяти источников на каждую из двух персон.');
  const selected = sourceGrants.flat();
  if (selected.length && !sourcePort)
    throw new Error('Для выбранных источников не настроен source port.');
  if (new Set(selected.map((grant) => grant.id)).size !== selected.length)
    throw new Error('Один source grant нельзя повторно использовать в одном эксперименте.');
  selected.forEach(validateSourceGrant);
  validateExperiment(input, [sourceGrants[0].length > 0, sourceGrants[1].length > 0]);
  const repetitions = options.repetitions ?? 1;
  const order = buildTrialOrder(repetitions);
  const task = input.task;
  const total = 8 + repetitions * 5;
  let completed = 0;
  const traces: StageTrace[] = [];
  const notify = (entry: StageTrace) => {
    const index = traces.findIndex((item) => item.id === entry.id);
    if (index < 0) traces.push(entry);
    else traces[index] = entry;
    options.onTrace?.(structuredClone(entry));
  };
  async function stage<T>(
    id: string,
    component: ComponentId,
    person: number | null,
    label: string,
    run: (provider: IntelligenceProvider) => Promise<T>,
  ): Promise<T> {
    options.signal?.throwIfAborted();
    const adapter = providers[settings.components[component]];
    const attempts: InvocationAttempt[] = [];
    let previous: IntelligenceTrace | undefined;
    const emit = (message: string) =>
      options.onProgress?.({
        id,
        stage: component,
        person,
        completed,
        total,
        message,
      });
    emit(label);
    const save = () => {
      if (previous)
        notify({ id, stage: component, person, label, trace: previous, attempts: [...attempts] });
    };
    const provider: IntelligenceProvider = {
      id: adapter.id,
      async run(request, signal) {
        if (attempts.length) {
          const last = attempts.at(-1)!;
          attempts[attempts.length - 1] = {
            ...last,
            status: request['format-attempt'] === 2 ? 'format-rejected' : 'accepted',
          };
          if (request['format-attempt'] === 2) emit(`${label} · исправление формата`);
        }
        const started = performance.now();
        try {
          const result = await adapter.run(
            {
              ...request,
              model: settings.aliases[component],
              'source-context': selected.length > 0 || request['source-context'] === true,
            },
            signal,
          );
          const elapsed = duration(result.trace);
          attempts.push({
            index: attempts.length + 1,
            status: 'returned',
            'wall-time-ms': elapsed,
            error: null,
          });
          const combined: IntelligenceTrace = {
            ...result.trace,
            'requested-model': settings.aliases[component],
            'requested-effort': request.effort,
            'resolved-model': result.trace['resolved-model'] ?? null,
            'applied-effort': result.trace['applied-effort'] ?? null,
            'cli-version': result.trace['cli-version'] ?? null,
            'active-reasoning-ms': null,
            'reasoning-ms': (previous ? duration(previous) : 0) + elapsed,
            'wall-time-ms': (previous ? duration(previous) : 0) + elapsed,
            'terminal-ms': (previous?.['terminal-ms'] ?? 0) + result.trace['terminal-ms'],
            reasoning: [
              ...(previous?.reasoning ?? []),
              ...(result.trace.reasoning ?? []).map((block) => ({
                ...block,
                id: `${attempts.length}:${block.id}`,
              })),
            ],
            'reasoning-truncated': Boolean(
              previous?.['reasoning-truncated'] || result.trace['reasoning-truncated'],
            ),
          };
          previous = combined;
          save();
          return { ...result, trace: combined };
        } catch (failure) {
          const partial = failure instanceof IntelligenceRunError ? failure.trace : undefined;
          const elapsed = partial ? duration(partial) : Math.max(0, performance.now() - started);
          const message = failure instanceof Error ? failure.message : String(failure);
          attempts.push({
            index: attempts.length + 1,
            status: 'failed',
            'wall-time-ms': elapsed,
            error: message,
          });
          previous = {
            ...partial,
            provider: adapter.id,
            'reasoning-ms': (previous ? duration(previous) : 0) + elapsed,
            'wall-time-ms': (previous ? duration(previous) : 0) + elapsed,
            'active-reasoning-ms': null,
            'requested-model': settings.aliases[component],
            'requested-effort': request.effort,
            'resolved-model': partial?.['resolved-model'] ?? null,
            'applied-effort': partial?.['applied-effort'] ?? null,
            'cli-version': partial?.['cli-version'] ?? null,
            'terminal-ms': (previous?.['terminal-ms'] ?? 0) + (partial?.['terminal-ms'] ?? 0),
            reasoning: [
              ...(previous?.reasoning ?? []),
              ...(partial?.reasoning ?? []).map((block) => ({
                ...block,
                id: `${attempts.length}:${block.id}`,
              })),
            ],
            'reasoning-truncated': Boolean(
              previous?.['reasoning-truncated'] || partial?.['reasoning-truncated'],
            ),
            summary: `Запрос не завершён: ${message}`,
          };
          save();
          throw failure;
        }
      },
    };
    try {
      const result = await run(provider);
      const last = attempts.at(-1);
      if (last) attempts[attempts.length - 1] = { ...last, status: 'accepted' };
      completed++;
      save();
      emit(`${label} · готово`);
      return result;
    } catch (failure) {
      const last = attempts.at(-1);
      if (last?.status === 'returned')
        attempts[attempts.length - 1] = {
          ...last,
          status:
            failure instanceof Error &&
            failure.message.includes('модель дважды вернула неподходящий формат JSON')
              ? 'format-rejected'
              : 'accepted',
        };
      save();
      throw failure;
    }
  }

  const generated = await stage('rubric', 'brain-analyzator', null, 'Критерии задачи', (provider) =>
    createRubric(task, provider, options.signal),
  );
  const protocol: ExperimentProtocol = freeze({
    version: 2,
    task,
    rubric: generated.rubric,
    repetitions,
    'participant-order': order,
    'model-settings': Object.fromEntries(
      componentIds.map((component) => [
        component,
        {
          provider: settings.components[component],
          'requested-model': settings.aliases[component],
          'requested-effort':
            component === 'stalker' || component === 'person-fabric' ? 'medium' : 'high',
        },
      ]),
    ),
    'format-attempts': 2,
    'stage-timeout-ms': 600000,
    'prompt-version': 'bsc-v2.3-source-identity',
    'created-at': new Date().toISOString(),
    'source-snapshots': [
      sourceGrants[0].map((grant) => grant['snapshot-id']),
      sourceGrants[1].map((grant) => grant['snapshot-id']),
    ],
  });
  await reviewProtocol(protocol, options);
  const prepared: { report: StalkerReport | null; base: PersonProfile; profile: PersonProfile }[] =
    [];
  for (const [index, person] of input.people.entries()) {
    const { report } = await stage(
      `sources-${index}`,
      'stalker',
      index,
      'Наблюдения и источники',
      async (provider) => {
        const grants = sourceGrants[index]!;
        if (!grants.length) return collectPerson(person, provider, options.signal);
        const collections: SourceCollection[] = [];
        for (const grant of grants) {
          const collection = await collectSource(
            grant,
            person,
            provider,
            sourcePort!,
            options.signal,
          );
          if (collection.status === 'cancelled') {
            options.signal?.throwIfAborted();
            throw new Error('Сбор выбранного источника отменён.');
          }
          collections.push(collection);
        }
        return analyzeSourceCollections(person, collections, provider, sourcePort!, options.signal);
      },
    );
    const { profile } = await stage(
      `profile-${index}`,
      'person-fabric',
      index,
      'Устойчивый профиль',
      (provider) =>
        fabricatePerson(
          report,
          person,
          index === 0 ? 'a' : 'b',
          index === 0 ? 'turquoise' : 'violet',
          provider,
          options.signal,
        ),
    );
    prepared.push({ report, base: freeze(profile), profile });
  }
  const neutral = freeze(neutralProfile());
  prepared.push({ report: null, base: neutral, profile: neutral });
  for (const [index, person] of prepared.entries()) {
    const { profile } = await stage(
      `adapt-${index}`,
      'person-fabric',
      index,
      'Стратегия для задачи',
      (provider) => adaptPerson(person.base, task, provider, options.signal),
    );
    person.profile = freeze(profile);
  }
  const trials: TrialResult[] = order.flatMap((ids, round) =>
    ids.map((id) => ({
      id: `${round + 1}-${id}`,
      round: round + 1,
      'person-id': id,
      status: 'pending',
      attempt: null,
      score: null,
      evaluations: [],
      error: null,
    })),
  );
  let status: ExperimentResult['status'] = 'complete';
  let activeTrial: TrialResult | undefined;
  const replace = (next: TrialResult) => {
    trials[trials.findIndex((item) => item.id === next.id)] = next;
  };
  try {
    for (let round = 1; round <= repetitions; round++) {
      for (const planned of trials.filter((item) => item.round === round)) {
        activeTrial = planned;
        const index = prepared.findIndex((person) => person.profile.id === planned['person-id']);
        const participant = prepared[index]!;
        const attempt = await stage(
          `solve-${planned.id}`,
          'person',
          index,
          `Решение · прогон ${round}`,
          (provider) => personRuntime.solve(participant.profile, task, provider, options.signal),
        );
        replace({ ...planned, status: 'answered', attempt });
        activeTrial = undefined;
      }
      const answers = trials
        .filter((item) => item.round === round)
        .map((item) => ({
          id: item['person-id'],
          answer: item.attempt!.answer,
        }));
      for (const [pass, ordered] of [answers, [...answers].reverse()].entries()) {
        const { evaluations } = await stage(
          `judge-${round}-${pass}`,
          'brain-analyzator',
          null,
          `Слепая оценка · прогон ${round} · порядок ${pass + 1}`,
          (provider) => evaluateAnswers(task, protocol.rubric, ordered, provider, options.signal),
        );
        for (const evaluation of evaluations) {
          const trial = trials.find(
            (item) => item.round === round && item['person-id'] === evaluation['person-id'],
          )!;
          replace({ ...trial, evaluations: [...trial.evaluations, evaluation] });
        }
      }
      for (const trial of trials.filter((item) => item.round === round)) {
        replace({
          ...trial,
          status: 'evaluated',
          score: scoreTrial(trial['person-id'], protocol.rubric, trial.evaluations),
        });
      }
    }
  } catch (failure) {
    status = options.signal?.aborted ? 'cancelled' : 'incomplete';
    const message = failure instanceof Error ? failure.message : String(failure);
    if (activeTrial) replace({ ...activeTrial, status: 'failed', error: message });
    for (const trial of trials) {
      if (trial.status === 'pending') replace({ ...trial, status: 'skipped', error: message });
      else if (trial.status === 'answered') replace({ ...trial, error: message });
    }
  }
  const participants = prepared.map((person, index): ExperimentPerson => {
    const ownTrials = trials.filter((trial) => trial['person-id'] === person.profile.id);
    return {
      report: person.report,
      'base-profile': person.base,
      profile: person.profile,
      attempt: ownTrials.find((trial) => trial.attempt)?.attempt ?? null,
      score: summarizeScores(
        person.profile.id,
        ownTrials.flatMap((trial) => (trial.score ? [trial.score] : [])),
        repetitions,
      ),
      trials: ownTrials,
      artifacts: personaArtifacts(person.profile),
      'baseline-difference': null,
      'preparation-wall-time-ms': traces
        .filter((item) => item.person === index && item.stage !== 'person')
        .reduce((sum, item) => sum + duration(item.trace), 0),
      'solution-wall-time-ms': traces
        .filter((item) => item.person === index && item.stage === 'person')
        .reduce((sum, item) => sum + duration(item.trace), 0),
    };
  });
  const control = participants[2]!;
  const people = participants.slice(0, 2).map((person) => ({
    ...person,
    'baseline-difference':
      person.score.interval && control.score.interval
        ? difference(person.score.interval, control.score.interval)
        : null,
  }));
  const a = people[0]!.score;
  const b = people[1]!.score;
  const eligible =
    status === 'complete' &&
    repetitions === 3 &&
    participants.every((person) => person.score['comparison-eligible']);
  const summary =
    status !== 'complete'
      ? 'Серия не завершена. Баллы относятся только к оценённым ответам; итоговое сравнение недоступно.'
      : repetitions === 1
        ? 'Предварительный эксперимент: один прогон. Баллы описывают выполнение критериев, устойчивость не установлена.'
        : eligible
          ? 'Описательное сравнение трёх прогонов в одинаковых условиях. Это не доказательство статистического превосходства.'
          : 'Сравнение ограничено: есть непроверенные критерии, расхождение оценок или невыполненные обязательные требования.';
  const result: ExperimentResult = {
    'schema-version': 3,
    'scoring-version': 2,
    status,
    input,
    protocol,
    people,
    control,
    trials,
    comparison: {
      summary,
      'comparison-eligible': eligible,
      difference: a.interval && b.interval ? difference(a.interval, b.interval) : null,
    },
    traces,
    'finished-at': new Date().toISOString(),
  };
  people.forEach((person) => options.onPerson?.(person));
  options.onProgress?.({
    id: 'complete',
    stage: 'complete',
    person: null,
    completed,
    total,
    message: summary,
  });
  return result;
}
