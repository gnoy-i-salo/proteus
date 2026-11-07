import { createSignal } from 'solid-js';
import styles from './return-to-app.module.scss';

export function ReturnToApp(props: { readonly deepLink: string }) {
  const [attempted, setAttempted] = createSignal(false);

  return (
    <div class={styles['returnToApp']}>
      <a
        class={styles['returnToApp__link']}
        href={props.deepLink}
        onClick={() => setAttempted(true)}
        referrerpolicy="no-referrer"
        aria-describedby="handoff-status"
        data-testid="return-to-proteus"
      >
        Вернуться в PROTEUS
        <span class={styles['returnToApp__arrow']} aria-hidden="true">
          ↗
        </span>
      </a>
      <p
        id="handoff-status"
        class={styles['returnToApp__status']}
        role="status"
        aria-live="polite"
        aria-atomic="true"
      >
        {attempted()
          ? 'Запрос на открытие PROTEUS отправлен. Если приложение не открылось, нажмите кнопку ещё раз.'
          : 'Браузер может запросить разрешение открыть приложение.'}
      </p>
    </div>
  );
}
