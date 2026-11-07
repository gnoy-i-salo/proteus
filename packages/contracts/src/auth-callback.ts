export const AUTH_CALLBACK_VERSION = 1;

export type AuthCallbackResult =
  | { readonly status: 'idle' }
  | { readonly status: 'success'; readonly deepLink: string }
  | {
      readonly status: 'error';
      readonly reason: 'access-denied' | 'provider-error';
      readonly deepLink: string;
    }
  | {
      readonly status: 'invalid';
      readonly reason:
        'invalid-query' | 'unexpected-parameter' | 'invalid-field' | 'ambiguous-callback';
    };

// These are application limits, not claims about VK's maximum field sizes.
const parameterLimits: Readonly<Record<string, number>> = {
  code: 4096,
  device_id: 512,
  state: 512,
  type: 7,
  expires_in: 10,
  ext_id: 512,
  error: 128,
  error_description: 1024,
};
const unsafeCharacters = /[\p{Cc}\p{Cf}\p{Cs}]/u;

function opaque(value: string | null): value is string {
  return value !== null && value.length > 0 && !/\s/u.test(value);
}

function handoff(fields: Readonly<Record<string, string>>): string {
  const query = new URLSearchParams({ v: String(AUTH_CALLBACK_VERSION), ...fields });
  return `proteus://oauth/vk?${query.toString()}`;
}

/**
 * Validates a VK HTTPS callback query and creates the fixed native handoff v1.
 * The success query requires code, device_id and state. Optional code_v2 type,
 * positive expires_in and ext_id are validated but never forwarded. On errors,
 * only state and a normalized error are forwarded; provider descriptions are discarded.
 * Unknown parameters, tokens and routing overrides are rejected, not silently ignored.
 *
 * This checks structure only. Rust must match state to a pending transaction,
 * enforce expiry and one-time use, and exchange the code with its stored PKCE verifier.
 * deepLink contains a short-lived authorization code: keep it in memory, never log it.
 */
export function parseVkAuthCallback(search: string): AuthCallbackResult {
  if (search.length > 16384 || search.includes('#'))
    return { status: 'invalid', reason: 'invalid-query' };
  const query = search.startsWith('?') ? search.slice(1) : search;
  if (!query) return { status: 'idle' };
  try {
    // URLSearchParams tolerates malformed percent escapes and invalid UTF-8.
    // Validate first so it cannot silently replace a code or state character.
    if (
      unsafeCharacters.test(decodeURIComponent(query.replace(/\+/gu, ' '))) ||
      query.split('&').some((part) => !part || !part.includes('='))
    )
      return { status: 'invalid', reason: 'invalid-query' };
  } catch {
    return { status: 'invalid', reason: 'invalid-query' };
  }

  const parameters = new URLSearchParams(query);
  const invalid = validateParameters(parameters);
  if (invalid) return invalid;

  const state = parameters.get('state');
  if (!opaque(state)) return { status: 'invalid', reason: 'invalid-field' };

  if (parameters.has('error')) {
    if (['code', 'device_id', 'type', 'expires_in', 'ext_id'].some((key) => parameters.has(key)))
      return { status: 'invalid', reason: 'ambiguous-callback' };
    const denied = parameters.get('error') === 'access_denied';
    return {
      status: 'error',
      reason: denied ? 'access-denied' : 'provider-error',
      deepLink: handoff({ error: denied ? 'access_denied' : 'provider_error', state }),
    };
  }

  if (parameters.has('error_description'))
    return { status: 'invalid', reason: 'ambiguous-callback' };
  const code = parameters.get('code');
  const deviceId = parameters.get('device_id');
  const expiresIn = parameters.get('expires_in');
  if (
    !opaque(code) ||
    !opaque(deviceId) ||
    (parameters.has('type') && parameters.get('type') !== 'code_v2') ||
    (parameters.has('ext_id') && !opaque(parameters.get('ext_id'))) ||
    (expiresIn !== null && !/^[1-9]\d{0,9}$/u.test(expiresIn))
  )
    return { status: 'invalid', reason: 'invalid-field' };

  return { status: 'success', deepLink: handoff({ code, device_id: deviceId, state }) };
}

function validateParameters(
  parameters: URLSearchParams,
): Extract<AuthCallbackResult, { status: 'invalid' }> | null {
  const seen = new Set<string>();
  for (const [key, value] of parameters) {
    if (seen.has(key)) return { status: 'invalid', reason: 'ambiguous-callback' };
    seen.add(key);
    if (!Object.hasOwn(parameterLimits, key))
      return { status: 'invalid', reason: 'unexpected-parameter' };
    if (value.length > parameterLimits[key]! || (!value && key !== 'error_description'))
      return { status: 'invalid', reason: 'invalid-field' };
  }

  return null;
}
