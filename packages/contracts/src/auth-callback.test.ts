import { describe, expect, it } from 'vitest';
import { AUTH_CALLBACK_VERSION, parseVkAuthCallback } from './index';

const success = '?code=authorization-code&device_id=device&state=expected-state';

describe('VK static callback contract v1', () => {
  it('distinguishes a page visit from a callback', () => {
    expect(parseVkAuthCallback('')).toEqual({ status: 'idle' });
    expect(parseVkAuthCallback('?')).toEqual({ status: 'idle' });
    expect(parseVkAuthCallback('?state=expected-state').status).toBe('invalid');
  });

  it('builds the fixed versioned handoff without claiming state verification', () => {
    expect(AUTH_CALLBACK_VERSION).toBe(1);
    expect(parseVkAuthCallback(success)).toEqual({
      status: 'success',
      deepLink:
        'proteus://oauth/vk?v=1&code=authorization-code&device_id=device&state=expected-state',
    });
    expect(parseVkAuthCallback(success.slice(1))).toEqual(parseVkAuthCallback(success));
  });

  it('accepts verified VK metadata but excludes it from the native handoff', () => {
    expect(
      parseVkAuthCallback(`${success}&type=code_v2&expires_in=600&ext_id=external-reference`),
    ).toEqual(parseVkAuthCallback(success));
  });

  it('encodes opaque values once without allowing them to add link parameters', () => {
    const query = new URLSearchParams({
      code: 'code+/=&return_to=https://example.org/%25',
      device_id: 'device+/%',
      state: 'state&error=access_denied',
    });
    const result = parseVkAuthCallback(`?${query}`);
    expect(result.status).toBe('success');
    if (result.status !== 'success') throw new Error('Expected a valid callback');
    const link = new URL(result.deepLink);
    expect([link.protocol, link.hostname, link.pathname, link.hash]).toEqual([
      'proteus:',
      'oauth',
      '/vk',
      '',
    ]);
    expect([...link.searchParams.entries()]).toEqual([['v', '1'], ...query.entries()]);
  });

  it('returns a safe cancellation and never includes the provider description', () => {
    const result = parseVkAuthCallback(
      '?error=access_denied&state=expected-state&error_description=%3Cscript%3Eprivate%3C%2Fscript%3E',
    );
    expect(result).toEqual({
      status: 'error',
      reason: 'access-denied',
      deepLink: 'proteus://oauth/vk?v=1&error=access_denied&state=expected-state',
    });
    expect(JSON.stringify(result)).not.toMatch(/script|private/iu);
  });

  it('normalizes unknown provider errors before handing them off', () => {
    expect(parseVkAuthCallback('?error=internal_private_error&state=expected-state')).toEqual({
      status: 'error',
      reason: 'provider-error',
      deepLink: 'proteus://oauth/vk?v=1&error=provider_error&state=expected-state',
    });
  });

  it.each([
    '?code=code&device_id=device',
    '?code=code&state=state',
    '?device_id=device&state=state',
    '?code=&device_id=device&state=state',
    '?code=code&device_id=&state=state',
    '?code=code&device_id=device&state=',
    '?error=access_denied',
    '?error=&state=state',
    '?error_description=description&state=state',
    `${success}&error=access_denied`,
    `${success}&error_description=unexpected`,
    '?error=access_denied&device_id=device&state=state',
    `${success}&type=token`,
    `${success}&expires_in=-1`,
    `${success}&expires_in=1.5`,
    `${success}&expires_in=0`,
    `${success}&expires_in=999999999999999999999999`,
    `${success}&ext_id=`,
  ])('rejects incomplete, mixed or invalid fields: %s', (query) => {
    expect(parseVkAuthCallback(query).status).toBe('invalid');
  });

  it.each([
    `${success}&code=second`,
    `${success}&%63ode=second`,
    `${success}&device_id=device`,
    `${success}&state=expected-state`,
    `${success}&expires_in=600&expires_in=600`,
    '?error=access_denied&error=access_denied&state=state',
  ])('rejects duplicate decoded parameter names: %s', (query) => {
    expect(parseVkAuthCallback(query)).toEqual({
      status: 'invalid',
      reason: 'ambiguous-callback',
    });
  });

  it.each([
    'return_to',
    'redirect_uri',
    'client_id',
    'token_endpoint',
    'access_token',
    'refresh_token',
    'id_token',
    'code_verifier',
    'client_secret',
    'provider',
    'v',
    'unexpected',
  ])('rejects non-contract parameters without exposing their values: %s', (key) => {
    const result = parseVkAuthCallback(`${success}&${key}=private-value`);
    expect(result).toEqual({ status: 'invalid', reason: 'unexpected-parameter' });
    expect(JSON.stringify(result)).not.toContain('private-value');
  });

  it.each([
    '?code=%&device_id=device&state=state',
    '?code=%2&device_id=device&state=state',
    '?code=%GG&device_id=device&state=state',
    '?code=%C3%28&device_id=device&state=state',
    '?code=%ED%A0%80&device_id=device&state=state',
    '?code=%00&device_id=device&state=state',
    '?code=code&device_id=device&state=state%0Ainjected',
    '?code=code&device_id=device&state=state%7F',
    '?code=code&device_id=device&state=state%C2%85',
    '?code=code&device_id=device&state=state%E2%80%AE',
    '?code=code&device_id=device&state=state#fragment',
    '?code=code&device_id=device&state=state\n',
    '?code=code&device_id=device&state=state&',
    '&',
  ])('rejects malformed encoding and unsafe query text: %s', (query) => {
    expect(parseVkAuthCallback(query).status).toBe('invalid');
  });

  it.each(['code', 'device_id', 'state'])('rejects whitespace in opaque %s', (key) => {
    const query = new URLSearchParams(success);
    query.set(key, 'two values');
    expect(parseVkAuthCallback(query.toString()).status).toBe('invalid');
  });

  it('bounds both encoded input and individual decoded fields', () => {
    expect(parseVkAuthCallback(`?${'x'.repeat(16385)}`).status).toBe('invalid');
    for (const [key, length] of [
      ['code', 4097],
      ['device_id', 513],
      ['state', 513],
    ] as const) {
      const query = new URLSearchParams(success);
      query.set(key, 'a'.repeat(length));
      expect(parseVkAuthCallback(query.toString()).status).toBe('invalid');
    }
    expect(
      parseVkAuthCallback(`?error=access_denied&state=state&error_description=${'x'.repeat(1025)}`)
        .status,
    ).toBe('invalid');
  });
});
