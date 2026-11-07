import { describe, expect, it } from 'vitest';
import {
  decodeLegacyConnectionConfig,
  decodeLegacyConnectionStatus,
  decodeLegacyImportedSource,
  encodeLegacyConnectionConfig,
} from './connection-wire-codec';

describe('connection native wire migration', () => {
  it('preserves exact public labels, statuses, challenges and unknown capability values', () => {
    expect(
      decodeLegacyConnectionStatus({
        provider: 'telegram',
        status: 'qr-required',
        label: 'Мария — сестра Ивана',
        message: '  текст\r\n👩🏽‍🔬',
        'client-id': 'client-id',
        'api-id': 123,
        'user-code': ' a-B ',
        'qr-code': 'data:image/svg+xml;base64,synthetic',
        'poll-after-ms': 1200,
        capabilities: ['archive-import', 'source-url'],
      }),
    ).toEqual({
      provider: 'telegram',
      status: 'qr-required',
      label: 'Мария — сестра Ивана',
      message: '  текст\r\n👩🏽‍🔬',
      clientId: 'client-id',
      apiId: 123,
      userCode: ' a-B ',
      qrCode: 'data:image/svg+xml;base64,synthetic',
      pollAfterMs: 1200,
      capabilities: ['archive-import', 'source-url'],
    });
  });
  it('does not fabricate missing login metadata', () => {
    expect(
      decodeLegacyConnectionStatus({
        provider: 'github',
        status: 'unconfigured',
        capabilities: [],
      }),
    ).toEqual({ provider: 'github', status: 'unconfigured', capabilities: [] });
  });
  it.each([
    {},
    { provider: null },
    { provider: 'telegram', 'api-id': 123, 'api-hash': 'synthetic-only' },
    {
      provider: 'instagram',
      token: 'synthetic-token-only',
      'client-id': null,
      'api-id': null,
      'api-hash': null,
    },
  ])('roundtrips transient configuration absent/null values %j', (value) => {
    const canonical = decodeLegacyConnectionConfig(value);
    expect(encodeLegacyConnectionConfig(canonical)).toEqual(value);
    expect(canonical).not.toHaveProperty('api-hash');
  });
  it('uses canonical config names while leaving one-use secret text exactly intact', () => {
    expect(
      decodeLegacyConnectionConfig({
        provider: 'telegram',
        'api-id': 123,
        'client-id': 'literal-client-id',
        'api-hash': 'synthetic-hash',
        token: null,
      }),
    ).toEqual({
      provider: 'telegram',
      apiId: 123,
      clientId: 'literal-client-id',
      apiHash: 'synthetic-hash',
      token: null,
    });
  });
  it('preserves cancelled import null and imported counts/warnings without granting access', () => {
    expect(decodeLegacyImportedSource(null)).toBeNull();
    expect(
      decodeLegacyImportedSource({
        id: 'import-id',
        label: '  Точное имя ',
        'document-count': 0,
        warnings: ['неизвестно\r\nsource-url'],
      }),
    ).toEqual({
      id: 'import-id',
      label: '  Точное имя ',
      documentCount: 0,
      warnings: ['неизвестно\r\nsource-url'],
    });
  });
  it.each([
    {
      provider: 'telegram',
      status: 'qr-required',
      capabilities: [],
      'qr-code': 'x',
      qrCode: 'x',
    },
    {
      provider: 'telegram',
      status: 'connected',
      capabilities: [],
      token: 'not-public',
    },
    {
      provider: 'telegram',
      status: 'connected',
      capabilities: [],
      label: null,
    },
    {
      provider: 'telegram',
      status: 'connected',
      capabilities: [],
      'poll-after-ms': -1,
    },
    {
      provider: 'telegram',
      status: 'connected',
      capabilities: [],
      'api-id': 2 ** 31,
    },
    { provider: 'telegram', status: 'invented', capabilities: [] },
    { provider: 'invented', status: 'connected', capabilities: [] },
    { provider: 'telegram', status: 'connected', capabilities: [1] },
  ])('rejects invalid status shapes without leaking input in errors %j', (value) => {
    expect(() => decodeLegacyConnectionStatus(value)).toThrow();
  });
  it.each([
    { provider: 'invented' },
    { provider: undefined },
    { 'api-id': 1.5 },
    { 'client-id': 'x', clientId: 'x' },
    { token: 'synthetic-secret', extra: true },
  ])('rejects corrupt/mixed transient config %j', (value) => {
    expect(() => decodeLegacyConnectionConfig(value)).toThrow();
  });
  it('rejects encoder aliases, import corruption and overlarge ephemeral QR data', () => {
    expect(() =>
      encodeLegacyConnectionConfig({
        provider: 'telegram',
        apiId: 12,
        'api-id': 12,
      }),
    ).toThrow();
    expect(() =>
      decodeLegacyImportedSource({
        id: 'id',
        label: 'name',
        'document-count': -1,
        warnings: [],
      }),
    ).toThrow();
    expect(() =>
      decodeLegacyImportedSource({
        id: 'id',
        label: 'name',
        'document-count': 1,
        documentCount: 1,
        warnings: [],
      }),
    ).toThrow();
    expect(() =>
      decodeLegacyConnectionStatus({
        provider: 'telegram',
        status: 'qr-required',
        capabilities: [],
        'qr-code': 'я'.repeat(65537),
      }),
    ).toThrow();
    expect(() => decodeLegacyConnectionConfig({ token: 'synthetic-secret', extra: true })).toThrow(
      /^Неверная схема подключения\.$/,
    );
  });
});
