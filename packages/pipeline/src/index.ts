import type {
  ExperimentInput,
  IntelligenceResult,
  IntelligenceTrace,
  PersonProfile,
  PersonScore,
  ProviderId,
} from '@proteus-int/contracts';
import type { IntelligenceProvider, ModelSettings, ComponentId } from '@proteus-int/intelligence';
import { collectPerson, validatePublicUrl } from '@proteus-int/stalker';
import type { StalkerReport } from '@proteus-int/stalker';
import { fabricatePerson } from '@proteus-int/person-fabric';
import { personaArtifacts } from '@proteus-int/person';
import { scorePerson } from '@proteus-int/brain-scoring';
import { analyzePeople } from '@proteus-int/brain-analyzator';
import type { BrainAnalysis } from '@proteus-int/brain-analyzator';
export interface ExperimentPerson {
  readonly report: StalkerReport;
  readonly profile: PersonProfile;
  readonly attempt: IntelligenceResult;
  readonly score: PersonScore;
  readonly artifacts: Readonly<Record<string, string>>;
}
export interface StageTrace {
  readonly stage: ComponentId;
  readonly person: number | null;
  readonly trace: IntelligenceTrace;
}
export interface ExperimentResult {
  readonly input: ExperimentInput;
  readonly people: readonly ExperimentPerson[];
  readonly comparison: BrainAnalysis;
  readonly 'finished-at': string;
  readonly traces: readonly StageTrace[];
}
export interface ProgressEvent {
  readonly stage: ComponentId | 'complete';
  readonly person: number | null;
  readonly completed: number;
  readonly total: number;
  readonly message: string;
  readonly trace?: IntelligenceTrace;
}
export interface RunOptions {
  readonly signal?: AbortSignal;
  readonly onProgress?: (event: ProgressEvent) => void;
  readonly onPerson?: (person: ExperimentPerson) => void;
  readonly onTrace?: (trace: StageTrace) => void;
}
export function validateExperiment(input: ExperimentInput): void {
  if (!input.task.trim() || input.task.length > 4000)
    throw new Error('Опишите общую задачу (до 4000 символов).');
  if (input.people.length !== 2) throw new Error('Нужны две персоны.');
  for (const person of input.people) {
    if (!person.label.trim() || person.label.length > 80)
      throw new Error('Укажите имя персоны (до 80 символов).');
    if (!person.context.trim() && !person['public-urls'].length)
      throw new Error('Добавьте публичную ссылку или контекст каждой персоны.');
    if (person.context.length > 4000 || person['public-urls'].length > 5)
      throw new Error('Максимум 5 ссылок и 4000 символов контекста на персону.');
    person['public-urls'].forEach(validatePublicUrl);
  }
}
export async function runExperiment(
  input: ExperimentInput,
  settings: ModelSettings,
  providers: Readonly<Record<ProviderId, IntelligenceProvider>>,
  options: RunOptions = {},
): Promise<ExperimentResult> {
  validateExperiment(input);
  let completed = 0;
  const traces: StageTrace[] = [];
  const provider = (stage: ComponentId, person: number | null): IntelligenceProvider => {
    const adapter = providers[settings.components[stage]];
    return {
      id: adapter.id,
      async run(request, signal) {
        const result = await adapter.run({ ...request, model: settings.aliases[stage] }, signal);
        const trace = { stage, person, trace: result.trace };
        traces.push(trace);
        options.onTrace?.(trace);
        return result;
      },
    };
  };
  const emit = (
    stage: ProgressEvent['stage'],
    person: number | null,
    message: string,
    trace?: IntelligenceTrace,
  ) => {
    options.onProgress?.({
      stage,
      person,
      completed,
      total: 7,
      message,
      ...(trace ? { trace } : {}),
    });
    options.signal?.throwIfAborted();
  };
  const people: ExperimentPerson[] = [];
  for (const [index, person] of input.people.entries()) {
    emit('stalker', index, 'Изучаем публичные источники и контекст');
    const collected = await collectPerson(person, provider('stalker', index), options.signal);
    completed++;
    emit('stalker', index, 'Факты и гипотезы собраны', collected.trace);
    emit('person-fabric', index, 'Создаём архетип и навыки');
    const fabricated = await fabricatePerson(
      collected.report,
      person,
      input.task,
      index === 0 ? 'a' : 'b',
      index === 0 ? 'turquoise' : 'violet',
      provider('person-fabric', index),
      options.signal,
    );
    completed++;
    emit('person-fabric', index, 'Архетип и навыки готовы', fabricated.trace);
    emit('person', index, 'Персона решает общую задачу');
    const attempt = await fabricated.runtime.solve(
      fabricated.profile,
      input.task,
      provider('person', index),
      options.signal,
    );
    const score = scorePerson({
      'person-id': fabricated.profile.id,
      'estimated-bsc': collected.report['estimated-bsc'],
      ...attempt.trace,
    });
    const result = {
      report: collected.report,
      profile: fabricated.profile,
      attempt,
      score,
      artifacts: personaArtifacts(fabricated.profile),
    };
    people.push(result);
    options.onPerson?.(result);
    completed++;
    emit('person', index, attempt.trace.summary, attempt.trace);
  }
  emit('brain-analyzator', null, 'Сравниваем подходы и результаты');
  const comparison = await analyzePeople(
    input.task,
    people.map((person) => person.attempt),
    people.map((person) => ({
      'person-id': person.profile.id,
      'estimated-bsc': person.report['estimated-bsc'],
      ...person.attempt.trace,
    })),
    provider('brain-analyzator', null),
    options.signal,
  );
  completed++;
  emit('complete', null, 'Сравнение готово');
  return { input, people, comparison, traces, 'finished-at': new Date().toISOString() };
}
