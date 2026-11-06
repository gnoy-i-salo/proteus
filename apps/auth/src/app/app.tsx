import type { AuthCallbackResult } from '@proteus-int/contracts';
import { AuthPage } from '../pages/auth';

export function App(props: { readonly callback: AuthCallbackResult }) {
  return <AuthPage callback={props.callback} />;
}
