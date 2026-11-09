import { createSignal, For, Show } from 'solid-js';
import type { LegacySeriesResult } from '@proteus-int/pipeline';
import { createTraceNavigation } from '../model/trace-navigation';
import { formatInterval, formatScore } from './experiment-report';
import { PagedText } from './paged-text';
import { TraceReader } from './trace-reader';
import styles from './experiment-history.module.scss';

const trialStatus = {
  pending: 'ожидает',
  answered: 'ответ получен',
  evaluated: 'оценён',
  failed: 'ошибка',
  skipped: 'пропущен',
};

export function HistoryLegacySeries(props: { readonly result: LegacySeriesResult }) {
  const [selected, setSelected] = createSignal(0);
  const [detail, setDetail] = createSignal('answer');
  const [round, setRound] = createSignal(1);
  const participants = () => [...props.result.people, props.result.control];
  const person = () => participants()[selected()]!;
  const trial = () => person().trials.find((value) => value.round === round());
  const traces = createTraceNavigation({
    stages: () => props.result.traces,
    running: () => false,
    activeStageId: () => undefined,
    failedStageId: () => undefined,
  });
  const text = () => {
    const value = person();
    if (detail() === 'answer')
      return trial()?.attempt?.answer ?? trial()?.error ?? 'Ответ не сохранён.';
    if (detail() === 'comparison')
      return [
        props.result.comparison.summary,
        `Разница A − B: ${formatInterval(props.result.comparison.difference)} п.п.`,
        ...participants().map((entry) => `${entry.profile.label}: ${formatScore(entry.score)}`),
      ].join('\n\n');
    if (detail() === 'protocol') {
      const protocol = props.result.protocol;
      return [
        protocol.task,
        `Дата: ${protocol.createdAt}\nПрогонов: ${protocol.repetitions}\nВерсия инструкций: ${protocol.promptVersion}`,
        ...protocol.rubric.criteria.map(
          (criterion) =>
            `${criterion.id} · ${criterion.label} · вес ${criterion.weight}${criterion.required ? ' · обязательный' : ''}\nНе выполнено: ${criterion.anchors.unmet}\nЧастично: ${criterion.anchors.partial}\nВыполнено: ${criterion.anchors.met}`,
        ),
        ...Object.entries(protocol.modelSettings).map(
          ([component, model]) =>
            `${component}: ${model.provider} · ${model.requestedModel || 'модель по умолчанию'} · ${model.requestedEffort}`,
        ),
      ].join('\n\n');
    }
    if (detail() === 'sources') {
      const report = value.report;
      if (!report) return 'Для нейтрального контроля персональный отчёт не сохранялся.';
      return [
        report.character,
        report.thinkingStyle,
        report.activity,
        report.behaviorSummary,
        ...report.evidence.map(
          (item) =>
            `${item.id} · ${item.status}\n${item.sourceUrl}\n${item.summary}\n${item.excerpt ?? 'Цитата отсутствует или изъята при сохранении/просмотре.'}\n${item.limitations.join('\n')}`,
        ),
      ].join('\n\n');
    }
    if (detail() === 'analysis')
      return value.trials
        .map((item) =>
          [
            `Прогон ${item.round} · ${trialStatus[item.status]} · ${formatInterval(item.score?.interval)}`,
            ...item.evaluations.flatMap((evaluation, index) =>
              evaluation.criteria.map(
                (criterion) =>
                  `Оценка ${index + 1} · ${criterion.criterionId} · ${criterion.status}\n${criterion.reason}\n${criterion.quote}`,
              ),
            ),
          ].join('\n\n'),
        )
        .join('\n\n');
    return [
      value.profile.archetype,
      ...value.profile.rules.map(
        (rule) =>
          `${rule.id} · ${rule.status} · ${rule.evidenceIds.join(', ')}\n${rule.instruction}\n${rule.limitations.join('\n')}`,
      ),
      ...value.profile.skills,
      ...value.profile.taskSkills.map((rule) => `${rule.id} · ${rule.status}\n${rule.instruction}`),
      ...(value.profile.styleProfile
        ? [...value.profile.styleProfile.voiceRules, ...value.profile.styleProfile.formatRules].map(
            (rule) => `${rule.status}\n${rule.instruction}`,
          )
        : []),
      ...(value.profile.styleExemplars ?? []).map((sample) => `${sample.context}\n${sample.text}`),
    ].join('\n\n');
  };
  return (
    <section
      class={styles.experimentHistory__original}
      data-palette={selected() === 1 ? 'violet' : 'turquoise'}
    >
      <nav class={styles.experimentHistory__tabs} aria-label="Участники сохранённого запуска">
        <For each={participants()}>
          {(value, index) => (
            <button
              type="button"
              aria-pressed={selected() === index()}
              onClick={() => setSelected(index())}
            >
              {value.profile.label}
            </button>
          )}
        </For>
      </nav>
      <h2>BSC v2 · {formatScore(person().score)}</h2>
      <p>Записанные оценки. Исторические условия сохранены; пересчёт не выполнялся.</p>
      <nav class={styles.experimentHistory__tabs} aria-label="Разделы исторической серии">
        <For
          each={[
            ['answer', 'Ответ'],
            ['analysis', 'Анализ'],
            ['profile', 'Профиль'],
            ['sources', 'Источники'],
            ['protocol', 'Протокол'],
            ['comparison', 'Сравнение'],
            ['traces', 'Intelligence traces'],
          ]}
        >
          {([id, label]) => (
            <button type="button" aria-pressed={detail() === id} onClick={() => setDetail(id!)}>
              {label}
            </button>
          )}
        </For>
      </nav>
      <Show when={detail() === 'answer'}>
        <label>
          Прогон{' '}
          <select
            aria-label="Прогон ответа"
            value={round()}
            onChange={(event) => setRound(Number(event.currentTarget.value))}
          >
            <For each={person().trials}>
              {(item) => (
                <option value={item.round}>
                  {item.round} · {trialStatus[item.status]}
                </option>
              )}
            </For>
          </select>
        </label>
        <p>
          Статус ответа: {trial() ? trialStatus[trial()!.status] : 'не сохранён'} · реплик:{' '}
          {trial()?.attempt?.reply?.messages.length ?? 'границы не сохранены'}
        </p>
      </Show>
      <Show when={detail() === 'protocol'}>
        <p>Исторический протокол v{props.result.protocol.version}</p>
      </Show>
      <Show when={detail() === 'traces'} fallback={<PagedText text={text()} />}>
        <TraceReader navigation={traces} running={false} />
      </Show>
    </section>
  );
}
