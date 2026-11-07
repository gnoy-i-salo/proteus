import { Show, untrack } from 'solid-js';
import type { ConnectionConfig, ConnectionStatus, SourceProvider } from '@proteus-int/contracts';
import { names } from '../model/connection-labels';
import styles from './connections-panel.module.scss';

export function ConfigurationForm(props: {
  provider: SourceProvider;
  status: ConnectionStatus;
  busy: boolean;
  onSubmit: (request: ConnectionConfig) => void;
}) {
  return (
    <form
      class={styles['connections-panel__form']}
      autocomplete="off"
      onSubmit={(event) => {
        event.preventDefault();
        const form = event.currentTarget;
        const fields = new FormData(form);
        const provider = props.provider;
        const request: ConnectionConfig =
          provider === 'telegram'
            ? {
                provider,
                apiId: Number(fields.get('api-id')),
                apiHash: String(fields.get('api-hash') ?? ''),
              }
            : provider === 'instagram'
              ? { provider, token: String(fields.get('token') ?? '') }
              : { provider, clientId: String(fields.get('client-id') ?? '').trim() };
        form.reset();
        props.onSubmit(request);
      }}
    >
      <Show when={props.provider === 'telegram'}>
        <label>
          API ID
          <input
            name="api-id"
            aria-label="API ID Telegram"
            type="number"
            min="1"
            max="2147483647"
            required
            value={untrack(() => props.status.apiId ?? '')}
            disabled={props.busy}
          />
        </label>
        <label>
          API hash
          <input
            name="api-hash"
            aria-label="API hash Telegram"
            type="password"
            maxLength={128}
            required
            autocomplete="off"
            disabled={props.busy}
          />
        </label>
      </Show>
      <Show when={props.provider === 'vk' || props.provider === 'github'}>
        <label>
          Client ID вашего приложения
          <input
            name="client-id"
            aria-label={`Client ID ${names[props.provider]}`}
            maxLength={128}
            required
            value={untrack(
              () => props.status.clientId ?? (props.provider === 'vk' ? '54881156' : ''),
            )}
            disabled={props.busy}
          />
        </label>
      </Show>
      <Show when={props.provider === 'instagram'}>
        <label>
          Токен из Meta App Dashboard
          <input
            name="token"
            aria-label="Токен Instagram"
            type="password"
            maxLength={4096}
            required
            autocomplete="off"
            disabled={props.busy}
          />
        </label>
      </Show>
      <button type="submit" disabled={props.busy}>
        {props.provider === 'instagram'
          ? 'Подключить Instagram'
          : `Сохранить настройки ${names[props.provider]}`}
      </button>
    </form>
  );
}
