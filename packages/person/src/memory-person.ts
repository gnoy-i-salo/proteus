import type {
  PersonProfile,
  OpenPersonaSessionRequest,
  PersonaMemoryPort,
  IntelligenceResult,
  IntelligenceTrace,
  PersonaContextLease,
  ModelMemoryAction,
  PersonaSimulationScope,
  SimulationMemoryEntry,
  PersonaTaskAdaptation,
} from '@proteus-int/contracts';
import {
  decodeOpenPersonaSessionRequest,
  decodeSkillDefinition,
  decodePersonaProfileSnapshot,
  parseMemoryAction,
  decodePersonaSimulationScope,
  decodePersonaTaskAdaptation,
} from '@proteus-int/contracts';
import type { IntelligenceProvider } from '@proteus-int/intelligence';
import { structured } from '@proteus-int/intelligence';
import { createPersonaModel } from './persona-model';
import type { MemoryRetrievalTrace } from './persona-model';
import { personTask } from './person-prompt';
import { parseReply } from './parse-reply';
export interface MemoryPersonInput {
  readonly profile: PersonProfile;
  readonly task: string;
  readonly request: OpenPersonaSessionRequest;
  readonly maxRetrievalSteps: number;
  readonly simulation: PersonaSimulationScope;
  readonly adaptation?: PersonaTaskAdaptation;
}
export type OpenPersonaMemory = (
  request: OpenPersonaSessionRequest,
  signal?: AbortSignal,
) => Promise<PersonaMemoryPort>;
export interface MemoryPersonResult extends IntelligenceResult {
  readonly traces: readonly IntelligenceTrace[];
  readonly retrievals: readonly MemoryRetrievalTrace[];
  readonly contextLeaseIds: readonly string[];
  readonly context: PersonaContextLease;
  readonly simulation: SimulationMemoryEntry;
}
function exactText(value: string, limit: number): void {
  const bytes = new TextEncoder().encode(value);
  if (
    typeof value !== 'string' ||
    bytes.length > limit ||
    new TextDecoder('utf-8', { fatal: true, ignoreBOM: true }).decode(bytes) !== value
  )
    throw new Error('Неверный текст Person.');
}
function validate(input: MemoryPersonInput) {
  const request = decodeOpenPersonaSessionRequest(input.request);
  const simulation = decodePersonaSimulationScope(input.simulation);
  const profile = structuredClone(input.profile);
  if (
    !profile.memory ||
    profile.memory.schemaVersion !== 1 ||
    profile.id !== request.personaId ||
    profile.memory.manifestId !== request.manifestId ||
    !Number.isSafeInteger(input.maxRetrievalSteps) ||
    input.maxRetrievalSteps < 0 ||
    input.maxRetrievalSteps > 64
  )
    throw new Error('Неверная область памяти Person.');
  exactText(input.task, 100000);
  exactText(profile.label, 512);
  if (profile.skills.length || profile.taskSkills.length || profile.sourceContext !== true)
    throw new Error('Person требует неизменённый task-free профиль памяти.');
  const snapshot = decodePersonaProfileSnapshot({
    schemaVersion: 1,
    label: profile.label,
    palette: profile.palette,
    archetype: profile.archetype,
    rules: profile.rules,
    sourceStatus: profile.sourceStatus,
    styleProfile: profile.styleProfile,
    styleExemplars: profile.styleExemplars,
  });
  const skills = profile.memory.skills.map(decodeSkillDefinition);
  const itemIds = [
    ...new Set([
      ...profile.rules.flatMap((rule) => rule.evidenceIds),
      ...profile.taskSkills.flatMap((rule) => rule.evidenceIds),
      ...skills.flatMap((skill) => skill.memoryItemIds),
      ...(profile.styleExemplars?.map((sample) => sample.id) ?? []),
    ]),
  ];
  const adaptation =
    input.adaptation === undefined
      ? undefined
      : decodePersonaTaskAdaptation(input.adaptation, {
          manifestId: request.manifestId,
          task: input.task,
          profile: snapshot,
        });
  const data = {
    ...personTask(profile, input.task),
    proceduralSkills: skills,
    ...(adaptation ? { taskAdaptation: adaptation } : {}),
  };
  exactText(JSON.stringify(data), 150000);
  return {
    request,
    profile,
    snapshot,
    skills,
    itemIds,
    data,
    simulation,
    maxRetrievalSteps: input.maxRetrievalSteps,
  };
}
function turn(
  value: Record<string, unknown>,
  canRetrieve: boolean,
  knowsEpisode: (id: string) => boolean,
) {
  if (value.action === 'reply') {
    if (
      !Array.isArray(value.styleConflicts) ||
      Object.keys(value).length !== 4 ||
      Object.keys(value).some(
        (key) => !['action', 'messages', 'summary', 'styleConflicts'].includes(key),
      )
    )
      throw new Error('Неверные поля ответа Person.');
    const reply = parseReply(value);
    exactText(reply.messages.join('\n\n'), 65536);
    return { action: 'reply' as const, reply };
  }
  if (!canRetrieve) throw new Error('Поиск недоступен: ответьте по уже предоставленным данным.');
  const action = parseMemoryAction(value);
  exactText(
    action.action === 'recall' ? action.query : action.episodeId,
    action.action === 'recall' ? 4096 : 256,
  );
  if (action.action === 'readEpisode' && /\p{Cc}/u.test(action.episodeId))
    throw new Error('Неверный идентификатор эпизода.');
  if (action.action === 'readEpisode' && !knowsEpisode(action.episodeId))
    throw new Error('Нельзя читать неизвестный эпизод.');
  return action;
}
export async function solveMemory(
  input: MemoryPersonInput,
  provider: IntelligenceProvider,
  open: OpenPersonaMemory,
  signal?: AbortSignal,
): Promise<MemoryPersonResult> {
  signal?.throwIfAborted();
  const selected = validate(input);
  const controller = new AbortController();
  const cancel = () => controller.abort(signal?.reason);
  signal?.addEventListener('abort', cancel, { once: true });
  const timeout = () => controller.abort(new Error('Истёк срок ответа Person.'));
  let timer = setTimeout(timeout, 600000);
  let memory: PersonaMemoryPort | undefined;
  let failure: { cause: unknown } | undefined;
  let result: MemoryPersonResult | undefined;
  try {
    memory = await open(selected.request, controller.signal);
    controller.signal.throwIfAborted();
    const model = createPersonaModel(selected.request, selected.itemIds, memory, provider);
    const manifest = model.session.manifest;
    if (
      manifest.profileVersion !== 'person-memory-profile:2' ||
      manifest.promptVersion !== 'person-fabric-memory:2' ||
      JSON.stringify(selected.snapshot) !== JSON.stringify(manifest.profileSnapshot) ||
      JSON.stringify(selected.skills) !== JSON.stringify(model.session.skills) ||
      selected.profile.memory!.memorySnapshotId !== manifest.memorySnapshotId
    )
      throw new Error('Профиль не соответствует манифесту Person.');
    clearTimeout(timer);
    const duration = Math.min(
      manifest.recallBudget.maxDurationMs,
      manifest.modelCallBudget.maxDurationMs,
      Date.parse(model.session.expiresAt) - Date.now(),
    );
    if (duration <= 0) throw new Error('Истёк срок ответа Person.');
    timer = setTimeout(timeout, duration);
    const seen = new Set<string>();
    let steps = 0;
    while (!result) {
      controller.signal.throwIfAborted();
      if (model.remainingCalls < 2)
        throw new Error('Недостаточно бюджета для ответа Person и исправления формата.');
      const canRetrieve =
        steps < selected.maxRetrievalSteps && model.canRetrieve && model.remainingCalls >= 4;
      const { data, trace, invocationId } = await structured(
        model.provider,
        'PERSON memory',
        '', // The bound provider supplies the versioned native-checked instructions.
        { ...selected.data, retrievalAllowed: canRetrieve },
        controller.signal,
        'high',
        (value) => {
          const action = turn(value, canRetrieve && model.canRetrieve, (id) =>
            model.knowsEpisode(id),
          );
          if (action.action !== 'reply' && seen.has(JSON.stringify(action)))
            throw new Error('Повторный поисковый шаг: выберите другой запрос или ответьте.');
          return action;
        },
        'persona-reply',
        true,
      );
      controller.signal.throwIfAborted();
      if (data.action === 'reply') {
        const answer = data.reply.messages.join('\n\n');
        const simulation = await model.publish(selected.simulation, answer, controller.signal);
        controller.signal.throwIfAborted();
        result = {
          simulation,
          ...(invocationId ? { invocationId } : {}),
          answer,
          reply: {
            version: 2,
            messages: data.reply.messages,
            styleConflicts: data.reply.conflicts,
          },
          trace: { ...trace, summary: data.reply.summary },
          traces: model.traces,
          retrievals: model.retrievals,
          contextLeaseIds: model.completedLeaseIds,
          context: model.context!,
        };
      } else {
        const action: ModelMemoryAction = data;
        seen.add(JSON.stringify(action));
        steps++;
        await model.retrieve(action, controller.signal);
      }
    }
  } catch (cause) {
    failure = { cause };
  }
  try {
    await memory?.close();
    if (!failure) controller.signal.throwIfAborted();
  } catch (cause) {
    if (failure)
      throw new AggregateError(
        [failure.cause, cause],
        'Ответ Person и закрытие сессии завершились ошибкой.',
        { cause },
      );
    throw cause;
  } finally {
    clearTimeout(timer);
    signal?.removeEventListener('abort', cancel);
  }
  if (failure) throw failure.cause;
  return result!;
}
