import type { PersonaRule } from '@proteus-int/contracts';
import type { StageTrace } from '@proteus-int/pipeline';
import type { StageArtifact, StageCall } from './stage-artifacts';
import { projectLiveStage } from './stage-artifacts';

const missingArtifact = 'Результат этого этапа не сохранён или не может быть однозначно связан.';
const missingPublished = 'Отдельный опубликованный текст для этого этапа не сохранён.';

function rulesText(rules: readonly PersonaRule[]): string {
  return rules
    .map((rule) =>
      [
        `${rule.id} · ${rule.status}`,
        rule.instruction,
        `Основания: ${rule.evidenceIds.join(', ') || 'не указаны'}`,
        `Ограничения: ${rule.limitations.join('; ') || 'не указаны'}`,
      ].join('\n'),
    )
    .join('\n\n');
}

export function publishedStageText(artifact: StageArtifact | undefined): string {
  if (!artifact) return missingArtifact;
  if (!('solution' in artifact)) return missingPublished;
  if (artifact.solution.status !== 'available') return missingArtifact;
  const answer = artifact.solution.value.answer;
  return answer.status === 'available' ? answer.value : 'Ответ не получен.';
}

export function stageResultText(artifact: StageArtifact | undefined): string {
  if (!artifact) return missingArtifact;
  if ('rubric' in artifact) {
    return [
      'Общая задача',
      artifact.rubric.task,
      'Критерии, зафиксированные до ответов',
      ...artifact.rubric.rubric.criteria.map((criterion) =>
        [
          `${criterion.id} · ${criterion.label}`,
          `Вес: ${criterion.weight}; обязательный: ${criterion.required ? 'да' : 'нет'}`,
          `Не выполнено: ${criterion.anchors.unmet}`,
          `Частично: ${criterion.anchors.partial}`,
          `Выполнено: ${criterion.anchors.met}`,
        ].join('\n'),
      ),
    ].join('\n\n');
  }
  if ('report' in artifact) {
    if (artifact.report.status !== 'available') return missingArtifact;
    const { report, sourceSnapshotIds } = artifact.report.value;
    return [
      'Отчёт STALKER. Утверждения модели сохраняют статус и ограничения; цитата сама по себе не подтверждает факт.',
      `Снимки источников: ${sourceSnapshotIds.join(', ') || 'не сохранены'}`,
      report.character,
      report.thinkingStyle,
      report.activity,
      report.behaviorSummary,
      ...report.evidence.map((item) =>
        [
          `${item.id} · ${item.kind} · ${item.status} · ${item.verification}`,
          item.summary,
          `Источник: ${item.sourceUrl || 'не указан'}`,
          `Цитата: ${item.excerpt ?? 'не представлена'}`,
          `Получено: ${item.retrievedAt ?? 'не установлено'}`,
          `Ограничения: ${item.limitations.join('; ') || 'не указаны'}`,
        ].join('\n'),
      ),
      'Записи поиска в памяти для этого результата не сохранены.',
    ].join('\n\n');
  }
  if ('profile' in artifact) {
    if (artifact.profile.status !== 'available') return missingArtifact;
    const profile = artifact.profile.value;
    const adaptation = artifact.request.kind === 'adaptation';
    return [
      adaptation ? 'Адаптация к общей задаче' : 'Базовый профиль без задачи',
      profile.label,
      profile.archetype,
      `Происхождение профиля: ${profile.sourceStatus}`,
      adaptation ? rulesText(profile.taskSkills) : rulesText(profile.rules),
    ].join('\n\n');
  }
  if ('solution' in artifact) {
    if (artifact.solution.status !== 'available') return missingArtifact;
    const { trial, reply, score } = artifact.solution.value;
    return [
      `Прогон ${trial.round} · ${trial.personId} · ${trial.status}`,
      reply.status === 'available'
        ? `Опубликованных сообщений: ${reply.value.messages.length}`
        : 'Структура сообщений не сохранена.',
      score.status === 'available'
        ? `BSC: ${score.value.interval.lower}–${score.value.interval.upper}`
        : 'Оценка ещё не рассчитана.',
      ...(trial.error === null ? [] : [`Ошибка: ${trial.error}`]),
      'Точный ответ доступен в представлении «Опубликованный текст».',
      'Записи поиска в памяти для этого результата не сохранены.',
    ].join('\n\n');
  }
  if (artifact.judgment.status !== 'available') return missingArtifact;
  const judgment = artifact.judgment.value;
  return [
    `Прогон ${judgment.round} · порядок оценки ${judgment.pass + 1}`,
    ...judgment.assessments.map((assessment) =>
      [
        assessment.personId,
        ...(assessment.evaluation?.criteria.map((criterion) =>
          [
            `${criterion.criterionId} · ${criterion.status}`,
            criterion.reason,
            `Цитата ответа: ${criterion.quote}`,
          ].join('\n'),
        ) ?? ['Оценка этого участника не сохранена.']),
      ].join('\n\n'),
    ),
  ].join('\n\n');
}

const callStatuses: Record<StageCall['status'], string> = {
  running: 'выполняется',
  validating: 'проверяется формат',
  completed: 'завершён',
  failed: 'ошибка',
  cancelled: 'отменён',
};

export function stageCallsText(artifact: StageArtifact | undefined, trace: StageTrace): string {
  const observations = trace.trace;
  const projection = artifact ?? projectLiveStage(trace);
  const calls = projection.calls;
  const lifecycle = projection.stage.lifecycle;
  const stageError = lifecycle.status === 'available' ? lifecycle.value.error : null;
  const details =
    calls?.status === 'available'
      ? calls.calls.map((call) =>
          [
            `Вызов ${call.index} · ${call.callId}`,
            `${call.trigger === 'initial' ? 'первичный вызов' : 'исправление формата'} · ${call.operation} · попытка формата ${call.formatAttempt}`,
            `Статус: ${callStatuses[call.status]}; время: ${call.wallTimeMs === null ? 'не измерено' : `${call.wallTimeMs} мс`}`,
            ...(call.error === null ? [] : [`Ошибка: ${call.error}`]),
            ...(call.validationError === null ? [] : [`Ошибка формата: ${call.validationError}`]),
          ].join('\n'),
        )
      : ['Метаданные вызовов отсутствуют или неоднозначны; связь не восстановлена.'];
  return [
    ...(details.length ? details : ['Вызовы модели не зарегистрированы.']),
    ...(stageError === null ? [] : [`Ошибка этапа: ${stageError}`]),
    'Параметры последнего наблюдения этапа (не всех вызовов)',
    `Провайдер: ${observations.provider}`,
    `Запрошенная модель: ${observations.requestedModel === undefined ? 'не сохранена' : observations.requestedModel || 'по умолчанию CLI'}`,
    `Наблюдаемая модель: ${observations.resolvedModel ?? 'не раскрыта'}`,
    `Запрошенное усилие: ${observations.requestedEffort ?? 'не сохранено'}`,
    `Наблюдаемое усилие: ${observations.appliedEffort ?? 'не раскрыто'}`,
    `Версия CLI: ${observations.cliVersion ?? 'не раскрыта'}`,
    'Время включает ожидание CLI и провайдера; активное время рассуждения не измерено.',
    'Полный исходный запрос и необработанный ответ для этого представления не сохранялись.',
  ].join('\n\n');
}
