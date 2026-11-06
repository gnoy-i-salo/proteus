import { render } from 'solid-js/web';
import { parseVkAuthCallback } from '@proteus-int/contracts';
import type { AuthCallbackResult } from '@proteus-int/contracts';
import { App } from './app';

export function mountAuthApp(callbackSearch: string | null): void {
  const callback: AuthCallbackResult =
    callbackSearch === null
      ? { status: 'invalid', reason: 'invalid-query' }
      : parseVkAuthCallback(callbackSearch);
  const root = document.getElementById('root');
  if (root) render(() => <App callback={callback} />, root);
}
