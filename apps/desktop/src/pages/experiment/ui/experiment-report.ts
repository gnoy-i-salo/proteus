import type {
  ExperimentProtocol,
  PersonaRule,
  ScoreInterval,
  ScoreSummary,
} from '@proteus-int/contracts';
import type { ExperimentPerson, ExperimentResult, StageTrace } from '@proteus-int/pipeline';

export function formatNumber(value: number): string {
  return value.toFixed(1).replace(/\.0$/u, '');
}
export function formatInterval(value: ScoreInterval | null | undefined): string {
  if (!value) return 'не оценено';
  return value.lower === value.upper
    ? formatNumber(value.lower)
    : `${formatNumber(Math.floor(value.lower * 10) / 10)}–${formatNumber(Math.ceil(value.upper * 10) / 10)}`;
}
export function formatScore(score: Pick<ScoreSummary, 'interval' | 'point'> | undefined): string {
  if (score?.interval && score.point === null && score.interval.lower === score.interval.upper)
    return `≈${formatInterval(score.interval)}`;
  return formatInterval(score?.interval);
}
export function traceMarker(
  stage: Pick<StageTrace, 'trace' | 'attempts' | 'lifecycle'>,
  running: boolean,
): string {
  if (stage.lifecycle) {
    const status = stage.lifecycle.status;
    return status === 'completed'
      ? '✓'
      : status === 'failed'
        ? '!'
        : status === 'cancelled'
          ? '×'
          : status === 'skipped'
            ? '—'
            : '◇';
  }
  const status = stage.attempts.at(-1)?.status;
  return status === 'accepted'
    ? '✓'
    : status === 'failed' || status === 'format-rejected'
      ? '!'
      : status || running
        ? '◇'
        : '—';
}
export function formatRetryCount(
  stages: readonly Pick<StageTrace, 'attempts' | 'lifecycle'>[],
): number | null {
  if (stages.some((stage) => !stage.lifecycle)) return null;
  return stages.reduce(
    (sum, stage) =>
      sum + (stage.lifecycle?.calls.filter((call) => call.trigger === 'formatRepair').length ?? 0),
    0,
  );
}
const sourceLabels = {
  'evidence-informed': 'Профиль по заявленным источникам',
  synthetic: 'Синтетический профиль',
  neutral: 'Нейтральный контроль',
};
export function sourceLabel(person: ExperimentPerson | undefined): string {
  return person ? sourceLabels[person.profile.sourceStatus] : '';
}
function rulesText(rules: readonly PersonaRule[]): string {
  return rules
    .map(
      (rule) =>
        `${rule.id} · ${rule.instruction}\nСтатус: ${rule.status}; основания: ${rule.evidenceIds.join(', ') || 'общая стратегия задачи'}\nОграничения: ${rule.limitations.join('; ') || 'Не указаны; независимой проверки нет.'}`,
    )
    .join('\n\n');
}
export function protocolText(protocol: ExperimentProtocol): string {
  return [
    'ПРОТОКОЛ BSC v2',
    `Общая задача: ${protocol.task}`,
    `Прогонов: ${protocol.repetitions}. Участники: A, B и нейтральный контроль.\nПорядок: ${protocol.participantOrder.map((round) => round.join(' → ')).join('; ')}`,
    'Критерии фиксируются до создания персон и ответов. Оценка смотрит только задачу и анонимные ответы в двух порядках. Неизвестность сохраняется диапазоном; BSC не измеряет интеллект человека.',
    'КРИТЕРИИ\n' +
      protocol.rubric.criteria
        .map(
          (criterion) =>
            `${criterion.id} · ${criterion.label}\nВес: ${criterion.weight}; обязательный: ${criterion.required ? 'да' : 'нет'}\nНе выполнено: ${criterion.anchors.unmet}\nЧастично: ${criterion.anchors.partial}\nВыполнено: ${criterion.anchors.met}`,
        )
        .join('\n\n'),
    'НАСТРОЙКИ\n' +
      Object.entries(protocol.modelSettings)
        .map(
          ([component, model]) =>
            `${component}: ${model.provider}; модель: ${model.requestedModel || 'по умолчанию CLI'}; запрошенное усилие: ${model.requestedEffort}`,
        )
        .join('\n'),
    `Попыток формата: ${protocol.formatAttempts}. Лимит этапа: ${protocol.stageTimeoutMs / 60000} мин.\nВерсия промптов: ${protocol.promptVersion}. Создан: ${protocol.createdAt}.`,
    'Одинаковые настройки не гарантируют одинаковое фактическое время или полный контроль CLI. Активное время рассуждения недоступно. Источники, ограничения и наблюдаемые параметры сохраняются в результате.',
  ].join('\n\n');
}
function answersText(person: ExperimentPerson): string {
  return person.trials
    .map(
      (trial) =>
        `ПРОГОН ${trial.round} · ${trial.status}\n${trial.attempt?.answer || 'Ответ не получен.'}${trial.error ? `\nОшибка: ${trial.error}` : ''}`,
    )
    .join('\n\n');
}
export function reportText(
  output: ExperimentResult,
  person: ExperimentPerson,
  tab: string,
): string {
  if (tab === 'answer') return answersText(person);
  if (tab === 'archetype')
    return `${person.profile.archetype}\n\n${sourceLabel(person)}. Это поведенческая симуляция, не установленный характер реального человека.\n\nПРАВИЛА УСТОЙЧИВОГО ПРОФИЛЯ\n${rulesText(person.profile.rules) || 'Недостаточно данных для характерных черт.'}\n\nАДАПТАЦИЯ К ЗАДАЧЕ\n${rulesText(person.profile.taskSkills)}`;
  if (tab === 'sources')
    return [
      'Независимая проверка источников не выполнена. «supported» означает заявленную моделью опору на источник. Цитата не доказывает истинность утверждения.',
      ...(person.report?.evidence.map(
        (item) =>
          `${item.id} · ${item.summary}\nСтатус: ${item.status}; тип утверждения: ${item.kind}; проверка: ${item.verification}\nИсточник: ${item.sourceUrl || 'Контекст пользователя / источник отсутствует'}\nЦитата, сообщённая моделью: ${item.excerpt || 'не представлена'}\nНаблюдаемая дата получения: ${item.retrievedAt || 'не установлена'}\nОграничения: ${item.limitations.join('; ') || 'не указаны'}`,
      ) ?? []),
      !person.report?.evidence.length
        ? 'Источников недостаточно. Используется синтетический профиль.'
        : '',
    ]
      .filter(Boolean)
      .join('\n\n');
  const score = person.score;
  const ownId = person.participantId ?? person.profile.id;
  const ownIndex =
    ownId === (output.control.participantId ?? output.control.profile.id)
      ? 2
      : output.people.findIndex((item) => (item.participantId ?? item.profile.id) === ownId);
  const ownTraces = output.traces.filter((trace) => trace.person === ownIndex);
  const retries = formatRetryCount(ownTraces);
  const required = { passed: 'выполнены', failed: 'не выполнены', unknown: 'не установлено' }[
    score.requiredStatus
  ];
  const assessments = person.trials
    .map(
      (trial) =>
        `ПРОГОН ${trial.round}: ${trial.status}; BSC ${formatInterval(trial.score?.interval)}\n${trial.evaluations.map((evaluation, pass) => `ПОРЯДОК ОЦЕНКИ ${pass + 1}\n${evaluation.criteria.map((criterion) => `${criterion.criterionId}: ${criterion.status}\nОснование: ${criterion.reason}\nЦитата ответа: ${criterion.quote || 'нет'}`).join('\n\n')}`).join('\n\n')}${trial.error ? `\nОшибка: ${trial.error}` : ''}`,
    )
    .join('\n\n');
  const observed = output.traces
    .map(
      (trace) =>
        `${trace.label}: ${trace.trace.provider}; запрошено ${trace.trace.requestedModel || 'по умолчанию CLI'}; наблюдаемая модель ${trace.trace.resolvedModel ?? 'не раскрыта'}; запрошенное усилие ${trace.trace.requestedEffort ?? 'не указано'}; применённое ${trace.trace.appliedEffort ?? 'не раскрыто'}; версия CLI ${trace.trace.cliVersion ?? 'не раскрыта'}`,
    )
    .join('\n');
  return [
    output.comparison.summary,
    `BSC v2: ${formatScore(score)} / 100.${score.point === null && score.interval ? ' Есть неопределённость или неполные прогоны.' : ''} Оценено ${score.evaluated} из ${score.planned} прогонов.\nРазброс: ${formatInterval(score.spread)}. Обязательные требования: ${required}.\nРасхождение оценок: ${score.judgeDisagreement ? 'есть' : 'нет'}. Это оценка выполнения критериев, не интеллекта.`,
    `${sourceLabel(person)}. ${person.profile.rules.flatMap((rule) => rule.limitations).join('; ') || 'Достоверность персональных черт не установлена.'}`,
    `НЕЙТРАЛЬНЫЙ КОНТРОЛЬ\nBSC v2: ${formatScore(output.control.score)} / 100. Оценено ${output.control.score.evaluated} из ${output.control.score.planned}.\nРазница с контролем: ${formatInterval(person.baselineDifference)} п.п. Разность не доказывает причинное влияние персоны.`,
    `ВРЕМЯ И ПОВТОРЫ\nПодготовка профиля и стратегии: ${formatNumber(person.preparationWallTimeMs / 1000)} с.\nРешение: ${formatNumber(person.solutionWallTimeMs / 1000)} с; повторных запросов формата: ${retries ?? 'неизвестно (старый формат)'}.\nВремя включает ожидание CLI/провайдера; активное время рассуждения не измерено. В BSC длительность не входит.`,
    'ОЦЕНКИ ПО КРИТЕРИЯМ\n' + assessments,
    'ОТВЕТЫ НЕЙТРАЛЬНОГО КОНТРОЛЯ\n' + answersText(output.control),
    protocolText(output.protocol),
    'НАБЛЮДАЕМЫЕ НАСТРОЙКИ CLI\n' + observed,
  ].join('\n\n');
}
