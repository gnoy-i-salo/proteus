import { Show } from 'solid-js';
import type { AuthCallbackResult } from '@proteus-int/contracts';
import { ReturnToApp } from '../../../features/return-to-app';
import { proteusLogo } from '../../../shared';
import styles from './auth-page.module.scss';

const messages = {
  idle: {
    title: 'Подключение VK',
    description: 'Начните подключение в PROTEUS',
    context: 'Это окно появится после входа через VK. Затем вы сможете вернуться в приложение.',
  },
  success: {
    title: 'Вернитесь в PROTEUS',
    description: 'Продолжите подключение в приложении',
    context: 'Ответ VK готов к передаче. Подключение ещё должно завершиться в PROTEUS.',
  },
  error: {
    title: 'Подключение не завершено',
    description: 'VK не подтвердил вход',
    context: 'Откройте PROTEUS и начните подключение заново, когда будете готовы.',
  },
  invalid: {
    title: 'Не удалось проверить ссылку',
    description: 'Начните подключение заново в PROTEUS',
    context: 'В этой ссылке недостаточно данных для безопасного возврата в приложение.',
  },
} as const;

export function AuthPage(props: { readonly callback: AuthCallbackResult }) {
  const message = () => messages[props.callback.status];
  const deepLink = () =>
    props.callback.status === 'success' || props.callback.status === 'error'
      ? props.callback.deepLink
      : undefined;

  return (
    <div class={styles['authPage']}>
      <header class={styles['authPage__brand']}>
        <span class={styles['authPage__wordmark']}>PROTEUS</span>
        <span class={styles['authPage__brandCaption']}>ONE TASK. MANY MINDS.</span>
      </header>
      <main class={styles['authPage__panel']} aria-labelledby="auth-title">
        <div class={styles['authPage__windowBar']} aria-hidden="true">
          <span>PROTEUS / CONNECTIONS</span>
          <span class={styles['authPage__pixels']}>
            <i />
            <i />
            <i />
          </span>
        </div>
        <div class={styles['authPage__content']}>
          <img class={styles['authPage__logo']} src={proteusLogo} width="112" height="91" alt="" />
          <p class={styles['authPage__eyebrow']}>VK · АВТОРИЗАЦИЯ</p>
          <h1 id="auth-title" class={styles['authPage__title']}>
            {message().title}
          </h1>
          <p class={styles['authPage__description']} data-testid="auth-status">
            {message().description}
          </p>
          <p class={styles['authPage__context']}>{message().context}</p>
          <Show when={deepLink()} keyed>
            {(href) => <ReturnToApp deepLink={href} />}
          </Show>
          <aside class={styles['authPage__help']} aria-label="Помощь с подключением">
            <span class={styles['authPage__helpMark']} aria-hidden="true">
              ?
            </span>
            <p>Для продолжения нужно настольное приложение PROTEUS с поддержкой подключения VK.</p>
          </aside>
        </div>
        <div class={styles['authPage__bottomLine']} aria-hidden="true" />
      </main>
      <footer class={styles['authPage__footer']}>Разные характеры. Общая основа.</footer>
    </div>
  );
}
