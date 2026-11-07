import { createSignal, createMemo, createEffect, on } from 'solid-js';
import type { ExperimentPerson } from '@proteus-int/pipeline';
import { PagedText } from './paged-text';
import styles from './experiment-page.module.scss';

export function ReplyReader(props: { person: ExperimentPerson }) {
  const [selectedMessage, setSelectedMessage] = createSignal(0);
  const replies = createMemo(() =>
    props.person.trials.flatMap((trial) => {
      const messages = trial.attempt?.reply?.messages ?? [
        trial.attempt?.answer || trial.error || 'Ответ не получен.',
      ];
      return messages.map((text, index) => ({
        text,
        round: trial.round,
        number: index + 1,
        total: messages.length,
      }));
    }),
  );
  createEffect(
    on(
      () => props.person.profile.id,
      () => setSelectedMessage(0),
    ),
  );
  const current = () => replies()[selectedMessage()];
  return (
    <section class={styles['experiment__reply']} aria-label="Ответы персоны">
      <p class={styles['experiment__replyLabel']}>Поведенческая симуляция</p>
      <div class={styles['experiment__replyBody']} data-testid="persona-reply-body">
        <PagedText
          text={current()?.text ?? ''}
          resetKey={`${props.person.profile.id}:${selectedMessage()}`}
        />
      </div>
      <nav class={styles['experiment__replyNavigation']} aria-label="Сообщения персоны">
        <button
          type="button"
          aria-label="Предыдущее сообщение"
          disabled={selectedMessage() === 0}
          onClick={() => setSelectedMessage(selectedMessage() - 1)}
        >
          ←
        </button>
        <span>
          Прогон {current()?.round} · Сообщение {current()?.number} / {current()?.total}
        </span>
        <button
          type="button"
          aria-label="Следующее сообщение"
          disabled={selectedMessage() >= replies().length - 1}
          onClick={() => setSelectedMessage(selectedMessage() + 1)}
        >
          →
        </button>
      </nav>
    </section>
  );
}
