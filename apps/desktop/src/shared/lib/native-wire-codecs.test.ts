import fixture from './fixtures/native-wire.json';
import { describe, expect, it } from 'vitest';
import { decodeCliEvent, decodeCliResult, encodeCliRequest } from './cli-wire-codec';
import {
  decodeConnectionStatus,
  decodeImportedSource,
  encodeConnectionConfig,
} from './connection-wire-codec';
import {
  decodeSourceGrant,
  decodeSourceResult,
  encodeSourceAction,
  encodeSourceGrantRequest,
} from './source-wire-codec';

const { sourceRequest, sourceGrant, sourceResult, cliRequest, cliResult, connectionStatus } =
  fixture;

describe('canonical embedded native transport', () => {
  it('preserves every native CLI field and only removes domain repair bookkeeping', () => {
    expect(encodeCliRequest({ ...cliRequest, formatAttempt: 2 })).toEqual(cliRequest);
    expect(decodeCliResult(cliResult)).toEqual(cliResult);
    expect(decodeCliEvent({ id: 'run-id', kind: 'started', elapsedMs: 0 })).toEqual({
      id: 'run-id',
      kind: 'started',
      elapsedMs: 0,
    });
    expect(decodeCliEvent(fixture.cliEvent)).toEqual(fixture.cliEvent);
  });
  it('keeps connection status public and transient optional config distinct from null', () => {
    expect(decodeConnectionStatus(connectionStatus)).toEqual(connectionStatus);
    expect(
      decodeConnectionStatus({ provider: 'vk', status: 'unconfigured', capabilities: [] }),
    ).toEqual({
      provider: 'vk',
      status: 'unconfigured',
      capabilities: [],
    });
    const config = {
      provider: 'telegram',
      apiId: 123,
      apiHash: 'synthetic-only',
      clientId: null,
      token: null,
    };
    expect(encodeConnectionConfig(config)).toEqual(config);
    expect(encodeConnectionConfig({})).toEqual({});
    expect(encodeConnectionConfig({ provider: null })).toEqual({ provider: null });
    expect(decodeImportedSource(null)).toBeNull();
    const imported = {
      id: 'source-id',
      label: 'Exact\r\n',
      documentCount: 0,
      warnings: ['source-url'],
    };
    expect(decodeImportedSource(imported)).toEqual(imported);
  });
  it('keeps source grants, nullable fields, holdout documents and source words exactly intact', () => {
    expect(encodeSourceGrantRequest(sourceRequest)).toEqual(sourceRequest);
    expect(decodeSourceGrant(sourceGrant)).toEqual(sourceGrant);
    expect(decodeSourceResult(sourceResult)).toEqual(sourceResult);
    const { authorId, from, to, ...requestWithoutDates } = sourceRequest;
    expect([authorId, from, to]).toEqual([null, null, null]);
    expect(encodeSourceGrantRequest(requestWithoutDates)).toEqual(requestWithoutDates);
    expect(() => decodeSourceGrant({ ...sourceGrant, authorId: undefined })).toThrow();
  });
  it.each([
    { action: 'describe' },
    { action: 'coverage' },
    { action: 'search', query: ' exact\r\n author-id ', limit: 7 },
    { action: 'collect', cursor: null, limit: 2 },
    { action: 'collect', limit: 2 },
    { action: 'read', documentIds: ['source-url', 'имя 👩🏽‍🔬'] },
  ])('keeps source action values and cursor presence %j', (value) => {
    expect(encodeSourceAction(value)).toEqual(value);
  });
  it.each([
    () => encodeCliRequest({ ...cliRequest, 'system-prompt': 'legacy' }),
    () => encodeCliRequest({ ...cliRequest, 'source-context': false }),
    () => decodeCliResult({ ...cliResult, trace: { ...cliResult.trace, 'reasoning-ms': 3 } }),
    () => decodeCliEvent({ id: 'run-id', kind: 'started', 'elapsed-ms': 3 }),
    () => decodeCliEvent({ id: 'run-id', kind: 'started', elapsedMs: 0, reasoning: null }),
    () => decodeConnectionStatus({ ...connectionStatus, 'qr-code': 'legacy' }),
    () => decodeConnectionStatus({ ...connectionStatus, token: 'synthetic-only' }),
    () => encodeConnectionConfig({ provider: 'telegram', 'api-hash': 'synthetic-only' }),
    () => decodeImportedSource({ id: 'id', label: 'label', 'document-count': 1, warnings: [] }),
    () => encodeSourceGrantRequest({ ...sourceRequest, 'resource-id': 'legacy' }),
    () =>
      encodeSourceGrantRequest({
        ...sourceRequest,
        budget: { ...sourceRequest.budget, 'max-steps': 3 },
      }),
    () => decodeSourceGrant({ ...sourceGrant, 'account-id': 'legacy' }),
    () => encodeSourceAction({ action: 'read', 'document-ids': ['id'] }),
    () => decodeSourceResult({ ...sourceResult, 'snapshot-id': 'legacy' }),
    () =>
      decodeSourceResult({
        ...sourceResult,
        coverage: { ...sourceResult.coverage, 'not-analyzed': 1 },
      }),
    () =>
      decodeSourceResult({
        ...sourceResult,
        documents: [{ ...sourceResult.documents[0], 'author-id': null }],
      }),
  ])('rejects legacy or mixed fields on live IPC without exposing payloads (%#)', (decode) => {
    expect(decode).toThrow();
  });
  it('keeps live size and numeric limits without promoting public answers to reasoning', () => {
    expect(() =>
      decodeCliEvent({
        id: 'run-id',
        kind: 'reasoning',
        elapsedMs: 3,
        reasoning: { id: 'r', kind: 'answer', text: 'answer' },
      }),
    ).toThrow();
    expect(() =>
      decodeCliEvent({
        id: 'run-id',
        kind: 'reasoning',
        elapsedMs: 3,
        reasoning: { id: 'r', kind: 'summary', text: 'я'.repeat(24577) },
      }),
    ).toThrow();
    expect(() =>
      decodeConnectionStatus({ ...connectionStatus, qrCode: 'я'.repeat(65537) }),
    ).toThrow();
    expect(() =>
      encodeSourceGrantRequest({
        ...sourceRequest,
        budget: { ...sourceRequest.budget, maxSteps: 2 ** 32 },
      }),
    ).toThrow();
    expect(() =>
      encodeSourceGrantRequest({
        ...sourceRequest,
        budget: { ...sourceRequest.budget, maxBytes: Number.MAX_SAFE_INTEGER + 1 },
      }),
    ).toThrow();
    expect(() =>
      decodeSourceResult({
        ...sourceResult,
        documents: [{ ...sourceResult.documents[0], text: 'x'.repeat(32 * 1024 * 1024) }],
      }),
    ).toThrow();
  });
});
