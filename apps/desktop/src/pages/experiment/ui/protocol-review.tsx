import { createSignal, createUniqueId, For, Show } from 'solid-js';
import type { ExperimentProtocol } from '@proteus-int/contracts';
import { TextWorkspace } from '../../../shared/ui/text-workspace/text-workspace';
import styles from './protocol-review.module.scss';
const views = [
  { id: 'criteria', label: 'Критерии' },
  { id: 'task', label: 'Задача' },
  { id: 'participants', label: 'Участники' },
  { id: 'settings', label: 'Настройки' },
] as const;
const anchors = [
  { id: 'unmet', label: 'Не выполнено' },
  { id: 'partial', label: 'Частично' },
  { id: 'met', label: 'Выполнено' },
] as const;
const componentLabels: Readonly<Record<string, string>> = {
  stalker: 'STALKER',
  'person-fabric': 'Person Fabric',
  person: 'Person',
  'brain-analyzator': 'Brain Analyzator',
};
const participantLabels: Readonly<Record<string, string>> = { a: 'A', b: 'B', neutral: 'Контроль' };
function moveTab(
  event: KeyboardEvent & { currentTarget: HTMLDivElement },
  current: number,
  count: number,
  select: (index: number) => void,
) {
  if (!['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key)) return;
  event.preventDefault();
  const next =
    event.key === 'Home'
      ? 0
      : event.key === 'End'
        ? count - 1
        : (current + (event.key === 'ArrowRight' ? 1 : -1) + count) % count;
  select(next);
  event.currentTarget.querySelectorAll<HTMLButtonElement>('[role="tab"]')[next]?.focus();
}
export function ProtocolReview(props: {
  readonly protocol: ExperimentProtocol;
  readonly onApprove: () => void;
  readonly onReturn: () => void;
}) {
  const id = createUniqueId();
  const [view, setView] = createSignal(0);
  const [criterionIndex, setCriterionIndex] = createSignal(0);
  const [anchorIndex, setAnchorIndex] = createSignal(0);
  const [componentIndex, setComponentIndex] = createSignal(0);
  const criterion = () => props.protocol.rubric.criteria[criterionIndex()];
  const anchor = () => anchors[anchorIndex()]!;
  const components = () => Object.entries(props.protocol.modelSettings);
  const component = () => components()[componentIndex()];
  return (
    <section class={styles['protocolReview']} aria-labelledby={`${id}-heading`}>
      <header class={styles['protocolReview__heading']}>
        <h2 id={`${id}-heading`}>Ожидает подтверждения</h2>
        <p>Критерии фиксируются до исследования, создания персон и ответов.</p>
      </header>
      <div
        class={styles['protocolReview__tabs']}
        role="tablist"
        aria-label="Разделы протокола"
        onKeyDown={(event) => moveTab(event, view(), views.length, setView)}
      >
        <For each={views}>
          {(item, index) => (
            <button
              type="button"
              role="tab"
              id={`${id}-tab-${item.id}`}
              aria-controls={`${id}-panel-${item.id}`}
              aria-selected={view() === index()}
              tabindex={view() === index() ? 0 : -1}
              onClick={() => setView(index())}
            >
              {item.label}
            </button>
          )}
        </For>
      </div>
      <div
        class={styles['protocolReview__body']}
        role="tabpanel"
        id={`${id}-panel-${views[view()]!.id}`}
        aria-labelledby={`${id}-tab-${views[view()]!.id}`}
      >
        <Show when={view() === 0}>
          <label class={styles['protocolReview__select']}>
            Критерий оценки
            <select
              aria-label="Критерий оценки"
              value={criterionIndex()}
              onChange={(event) => setCriterionIndex(Number(event.currentTarget.value))}
            >
              <For each={props.protocol.rubric.criteria}>
                {(item, index) => (
                  <option value={index()}>
                    {index() + 1}. {item.label}
                  </option>
                )}
              </For>
            </select>
          </label>
          <Show when={criterion()}>
            {(item) => (
              <>
                <div class={styles['protocolReview__criterion']}>
                  <strong>{item().label}</strong>
                  <span>
                    Вес: <b data-testid="criterion-weight">{String(item().weight)}</b>
                  </span>
                  <span>{item().required ? 'Обязательный' : 'Дополнительный'}</span>
                </div>
                <div
                  class={styles['protocolReview__tabs']}
                  role="tablist"
                  aria-label="Шкала критерия"
                  onKeyDown={(event) =>
                    moveTab(event, anchorIndex(), anchors.length, setAnchorIndex)
                  }
                >
                  <For each={anchors}>
                    {(item, index) => (
                      <button
                        type="button"
                        role="tab"
                        id={`${id}-anchor-${item.id}`}
                        aria-controls={`${id}-anchor-panel`}
                        aria-selected={anchorIndex() === index()}
                        tabindex={anchorIndex() === index() ? 0 : -1}
                        onClick={() => setAnchorIndex(index())}
                      >
                        {item.label}
                      </button>
                    )}
                  </For>
                </div>
                <div
                  class={styles['protocolReview__text']}
                  role="tabpanel"
                  id={`${id}-anchor-panel`}
                  aria-labelledby={`${id}-anchor-${anchor().id}`}
                >
                  <TextWorkspace
                    label={`Шкала: ${anchor().label}`}
                    value={item().anchors[anchor().id]}
                    resetKey={`${item().id}:${anchor().id}`}
                  />
                </div>
              </>
            )}
          </Show>
        </Show>
        <Show when={view() === 1}>
          <TextWorkspace
            label="Общая задача протокола"
            value={props.protocol.task}
            resetKey={props.protocol.createdAt}
          />
        </Show>
        <Show when={view() === 2}>
          <h3>A, B и нейтральный контроль</h3>
          <p>
            Одна задача и одна шкала для всех участников. Прогонов: {props.protocol.repetitions}.
          </p>
          <ol class={styles['protocolReview__order']}>
            <For each={props.protocol.participantOrder}>
              {(round, index) => (
                <li>
                  Прогон {index() + 1}:{' '}
                  {round.map((person) => participantLabels[person] ?? person).join(' → ')}
                </li>
              )}
            </For>
          </ol>
          <p>
            BSC — экспериментальная оценка ответа по этой шкале, а не измерение интеллекта человека.
          </p>
        </Show>
        <Show when={view() === 3}>
          <label class={styles['protocolReview__select']}>
            Компонент
            <select
              aria-label="Настройки компонента протокола"
              value={componentIndex()}
              onChange={(event) => setComponentIndex(Number(event.currentTarget.value))}
            >
              <For each={components()}>
                {([key], index) => <option value={index()}>{componentLabels[key] ?? key}</option>}
              </For>
            </select>
          </label>
          <Show when={component()}>
            {(entry) => (
              <dl class={styles['protocolReview__settings']}>
                <dt>Провайдер</dt>
                <dd>{entry()[1].provider}</dd>
                <dt>Запрошенная модель</dt>
                <dd>{entry()[1].requestedModel || 'По умолчанию CLI'}</dd>
                <dt>Запрошенное усилие</dt>
                <dd>{entry()[1].requestedEffort}</dd>
                <dt>Проверка формата</dt>
                <dd>До {props.protocol.formatAttempts} попыток</dd>
                <dt>Лимит этапа</dt>
                <dd>{props.protocol.stageTimeoutMs / 60000} минут</dd>
              </dl>
            )}
          </Show>
          <p>
            Это запрос к CLI. Фактическая модель и доступные данные вызова сохраняются в результате.
          </p>
        </Show>
      </div>
      <footer class={styles['protocolReview__actions']}>
        <button type="button" onClick={() => props.onReturn()}>
          Вернуться к задаче
        </button>
        <button
          type="button"
          disabled={!props.protocol.rubric.criteria.length}
          onClick={() => props.onApprove()}
        >
          Подтвердить и продолжить
        </button>
      </footer>
    </section>
  );
}
