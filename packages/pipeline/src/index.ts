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
    let previous: IntelligenceTrace | undefined;
    let attempt = 0;
    return {
      id: adapter.id,
      async run(request, signal) {
        if (attempt > 0) emit(stage, person, 'Исправляем формат ответа · повторная попытка');
        attempt++;
        const result = await adapter.run({ ...request, model: settings.aliases[stage] }, signal);
        const combined: IntelligenceTrace = {
          ...result.trace,
          'reasoning-ms': (previous?.['reasoning-ms'] ?? 0) + result.trace['reasoning-ms'],
          'terminal-ms': (previous?.['terminal-ms'] ?? 0) + result.trace['terminal-ms'],
          reasoning: [
            ...(previous?.reasoning ?? []),
            ...(result.trace.reasoning ?? []).map((block) => ({
              ...block,
              id: `${attempt}:${block.id}`,
            })),
          ],
          'reasoning-truncated': Boolean(
            previous?.['reasoning-truncated'] || result.trace['reasoning-truncated'],
          ),
        };
        previous = combined;
        const trace = { stage, person, trace: combined };
        const index = traces.findIndex((item) => item.stage === stage && item.person === person);
        if (index < 0) traces.push(trace);
        else traces[index] = trace;
        options.onTrace?.(trace);
        return { ...result, trace: combined };
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

export function parsePersonaPreset(data: string): ExperimentInput {
  if (new TextEncoder().encode(data).length > 131072) throw new Error('Пресет превышает 128 КиБ.');
  let value: unknown;
  try {
    value = JSON.parse(data);
  } catch {
    throw new Error('Файл .proteus содержит неверный JSON.');
  }
  if (!value || typeof value !== 'object') throw new Error('Неверный формат пресета.');
  const preset = value as Record<string, unknown>;
  if (preset['format'] !== 'proteus-personas' || preset['version'] !== 1)
    throw new Error('Неподдерживаемый формат или версия .proteus.');
  if (
    typeof preset['task'] !== 'string' ||
    !Array.isArray(preset['people']) ||
    preset['people'].length !== 2
  )
    throw new Error('Пресет должен содержать задачу и две персоны.');
  const people = preset['people'].map((item: unknown) => {
    if (!item || typeof item !== 'object') throw new Error('Неверная персона в пресете.');
    const person = item as Record<string, unknown>;
    if (
      typeof person['label'] !== 'string' ||
      typeof person['context'] !== 'string' ||
      !Array.isArray(person['public-urls']) ||
      !person['public-urls'].every((url: unknown) => typeof url === 'string')
    )
      throw new Error('Неверные поля персоны в пресете.');
    return {
      label: person['label'],
      context: person['context'],
      'public-urls': person['public-urls'] as string[],
    };
  });
  const input: ExperimentInput = { task: preset['task'], people: [people[0]!, people[1]!] };
  validateExperiment({ ...input, task: input.task.trim() ? input.task : 'Preset' });
  if (input.task.length > 4000) throw new Error('Задача превышает 4000 символов.');
  return input;
}
export function serializePersonaPreset(input: ExperimentInput): string {
  const clean = parsePersonaPreset(
    JSON.stringify({ format: 'proteus-personas', version: 1, ...input }),
  );
  return JSON.stringify({ format: 'proteus-personas', version: 1, ...clean }, null, 2);
}
