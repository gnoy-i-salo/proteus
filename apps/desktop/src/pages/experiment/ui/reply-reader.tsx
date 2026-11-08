import { For, Show } from 'solid-js';
import type { ModelCallLifecycle } from '@proteus-int/contracts';
import { TextWorkspace } from '../../../shared/ui/text-workspace/text-workspace';
import type { ReplyNavigation } from '../model/reply-navigation';
import styles from './experiment-page.module.scss';
import replyStyles from './reply-reader.module.scss';

const triggerLabels = {
  initial: 'первичный вызов',
  formatRepair: 'исправление формата',
} as const;
const callStatusLabels = {
  running: 'выполняется',
  validating: 'проверка',
  completed: 'завершён',
  failed: 'ошибка',
  cancelled: 'отменён',
} as const;

function callText(call: ModelCallLifecycle): string {
  const time = call.wallTimeMs === null ? 'время не измерено' : `${call.wallTimeMs} мс`;
  return `Вызов: ${triggerLabels[call.trigger]} · ${callStatusLabels[call.status]} · ${time}`;
}

export function ReplyReader(props: {
  readonly navigation: ReplyNavigation;
  readonly onOpenCall?: (stageId: string) => void;
  readonly onReturn?: () => void;
  readonly canReturn: boolean;
}) {
  const navigation = () => props.navigation;
  const quoteLocation = () => navigation().quoteLocation();
  const occurrence = () => navigation().quoteOccurrences()[navigation().quoteOccurrenceIndex()];
  const creatingCall = () => navigation().creatingCall();
  const callSummary = () => {
    const call = creatingCall();
    return call.kind === 'linked'
      ? callText(call.call)
      : 'Создавший вызов не связан: метаданные вызова не сохранены.';
  };
  const quoteSignature = () => {
    const target = navigation().quoteTarget();
    if (!target) return 'none';
    return `${target.round}:${navigation().quoteOccurrenceIndex()}`;
  };
  const atFirstMessage = () =>
    navigation().activeMessageIndex() === 0 &&
    (navigation().rounds()[0] ?? null) === navigation().activeRound();
  const atLastMessage = () => {
    const items = navigation().items();
    const rounds = navigation().rounds();
    return (
      navigation().activeMessageIndex() >= items.length - 1 &&
      navigation().activeRound() === rounds[rounds.length - 1]
    );
  };
  return (
    <section class={styles['experiment__reply']} aria-label="Ответы персоны">
      <p class={styles['experiment__replyLabel']}>Поведенческая симуляция</p>
      <div class={replyStyles.replyReader__selectors}>
        <label class={replyStyles.replyReader__selector}>
          Прогон
          <select
            aria-label="Прогон ответа"
            value={navigation().activeRound() ?? ''}
            onChange={(event) => navigation().selectRound(Number(event.currentTarget.value))}
          >
            <For each={navigation().rounds()}>
              {(round) => (
                <option value={round}>
                  Прогон {round}
                  <Show when={navigation().unpublishedRounds().has(round)}> · нет ответа</Show>
                </option>
              )}
            </For>
          </select>
        </label>
        <label class={replyStyles.replyReader__selector}>
          Сообщение
          <select
            aria-label="Сообщение ответа"
            value={navigation().activeMessageIndex()}
            onChange={(event) => navigation().selectMessage(Number(event.currentTarget.value))}
          >
            <For each={navigation().items()}>
              {(item) => (
                <option value={item.index}>
                  Сообщение {item.number} / {item.total}
                </option>
              )}
            </For>
          </select>
        </label>
      </div>
      <Show when={navigation().quoteTarget()}>
        <div class={replyStyles.replyReader__quote} role="status" aria-label="Переход к цитате">
          <Show
            when={quoteLocation()?.kind === 'found'}
            fallback={
              <span>
                {quoteLocation()?.kind === 'absent'
                  ? 'Цитата не найдена в опубликованном ответе.'
                  : 'Ответ не опубликован, цитата недоступна.'}
              </span>
            }
          >
            <span>
              Цитата из оценки · вхождение {navigation().quoteOccurrenceIndex() + 1} из{' '}
              {navigation().quoteOccurrences().length}
              <Show when={(occurrence()?.lastMessage ?? 0) > (occurrence()?.firstMessage ?? 0)}>
                {' '}
                · сообщения {(occurrence()?.firstMessage ?? 0) + 1}–
                {(occurrence()?.lastMessage ?? 0) + 1}
              </Show>
            </span>
            <span class={replyStyles.replyReader__quoteControls}>
              <button
                type="button"
                aria-label="Предыдущее вхождение"
                disabled={navigation().quoteOccurrenceIndex() === 0}
                onClick={() =>
                  navigation().setQuoteOccurrence(navigation().quoteOccurrenceIndex() - 1)
                }
              >
                ←
              </button>
              <button
                type="button"
                aria-label="Следующее вхождение"
                disabled={
                  navigation().quoteOccurrenceIndex() >= navigation().quoteOccurrences().length - 1
                }
                onClick={() =>
                  navigation().setQuoteOccurrence(navigation().quoteOccurrenceIndex() + 1)
                }
              >
                →
              </button>
            </span>
          </Show>
          <button type="button" onClick={() => navigation().clearQuote()}>
            Закрыть
          </button>
          <Show when={props.canReturn}>
            <button type="button" onClick={() => props.onReturn?.()}>
              ← Вернуться к анализу
            </button>
          </Show>
        </div>
      </Show>
      <div class={styles['experiment__replyBody']} data-testid="persona-reply-body">
        <TextWorkspace
          label="Текст"
          value={navigation().current()?.text ?? ''}
          resetKey={`${navigation().positionKey()}:${quoteSignature()}`}
          position={() => navigation().workspacePosition()}
          onPositionChange={(position) =>
            navigation().savePosition(navigation().positionKey(), position)
          }
        />
      </div>
      <div class={replyStyles.replyReader__call} role="group" aria-label="Создавший вызов">
        <span>{callSummary()}</span>
        <button
          type="button"
          disabled={creatingCall().kind !== 'linked' || !props.onOpenCall}
          onClick={() => {
            const call = creatingCall();
            if (call.kind === 'linked') props.onOpenCall?.(call.stage.id);
          }}
        >
          К вызову
        </button>
      </div>
      <nav class={styles['experiment__replyNavigation']} aria-label="Сообщения персоны">
        <button
          type="button"
          aria-label="Предыдущее сообщение"
          disabled={atFirstMessage()}
          onClick={() => navigation().stepMessage(-1)}
        >
          ←
        </button>
        <span>
          <Show
            when={navigation().current()}
            fallback="Ответы отсутствуют"
          >{`Прогон ${navigation().activeRound()} · Сообщение ${navigation().current()?.number} / ${navigation().current()?.total}`}</Show>
        </span>
        <button
          type="button"
          aria-label="Следующее сообщение"
          disabled={atLastMessage()}
          onClick={() => navigation().stepMessage(1)}
        >
          →
        </button>
      </nav>
    </section>
  );
}
