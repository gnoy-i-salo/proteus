import { describe, expect, it } from 'vitest';
import {
  decodeFabricContextRequest,
  decodeFabricSession,
  decodeOpenFabricSessionRequest,
} from './fabric-memory-codec';
import { fabricContextRequest, fabricRequest, fabricSession } from './testing/fabric-fixtures';

describe('native Fabric session and request readers', () => {
  it('retains explicit model choice, nullable period and lexical-only admission', () => {
    expect(decodeOpenFabricSessionRequest(fabricRequest)).toEqual(fabricRequest);
    expect(decodeFabricSession(fabricSession, fabricRequest)).toEqual(fabricSession);
    for (const patch of [
      { mode: 'hybrid' },
      { purpose: 'research' },
      { operation: 'generation' },
      { model: { ...fabricRequest.model, operation: 'generation' } },
    ])
      expect(() => decodeOpenFabricSessionRequest({ ...fabricRequest, ...patch })).toThrow();
    expect(() =>
      decodeOpenFabricSessionRequest({
        ...fabricRequest,
        recallBudget: { ...fabricRequest.recallBudget, maxCalls: 0 },
      }),
    ).toThrow();
  });
  it.each([
    { purpose: 'research' },
    { mode: 'hybrid' },
    { personaId: 'foreign' },
    { corpusSnapshotId: 'foreign' },
    { indexGeneration: 0 },
    { schemaVersion: 2 },
    { policyVersion: '' },
    { expiresAt: '2026-02-30T00:00:00.000Z' },
    { unknown: true },
  ])('rejects mismatched Fabric admissions: %j', (patch) => {
    expect(() => decodeFabricSession({ ...fabricSession, ...patch }, fabricRequest)).toThrow();
  });
  it('preserves exact input, item order and format retry without adding authority', () => {
    const value = { ...fabricContextRequest, itemIds: ['memory:2', 'memory:1'], formatAttempt: 2 };
    expect(decodeFabricContextRequest(value)).toEqual(value);
    expect(
      decodeFabricContextRequest({ ...value, itemIds: [], systemPrompt: '', task: '' }).itemIds,
    ).toEqual([]);
    expect(
      decodeFabricContextRequest({
        ...value,
        systemPrompt: 'я'.repeat(20000),
        task: '😀'.repeat(50000),
      }).task.length,
    ).toBe(100000);
  });
  it.each([
    { schemaVersion: 2 },
    { memorySnapshotId: '' },
    { itemIds: ['a', 'a'] },
    { itemIds: ['x\0'] },
    { itemIds: ['я'.repeat(129)] },
    { itemIds: Array.from({ length: 1001 }, (_, i) => `m:${i}`) },
    { formatAttempt: 0 },
    { formatAttempt: 3 },
    { systemPrompt: '\ud800' },
    { systemPrompt: 'я'.repeat(20001) },
    { task: '😀'.repeat(50001) },
    { futureTask: 'foreign' },
    { inputSha256: 'a'.repeat(64) },
    { receiptIds: [] },
    { model: 'foreign' },
    Object.fromEntries([['memory-snapshot-id', 'legacy']]),
  ])('rejects malformed or authority-bearing context input: %j', (patch) => {
    const input = { ...fabricContextRequest, ...patch };
    const before = structuredClone(input);
    expect(() => decodeFabricContextRequest(input)).toThrow();
    expect(input).toEqual(before);
  });
  it('requires every field of every versioned boundary', () => {
    for (const [value, decode] of [
      [fabricRequest, decodeOpenFabricSessionRequest],
      [fabricSession, (value: unknown) => decodeFabricSession(value, fabricRequest)],
      [fabricContextRequest, decodeFabricContextRequest],
    ] as const) {
      for (const key of Object.keys(value)) {
        const input: Record<string, unknown> = { ...value };
        delete input[key];
        expect(() => decode(input)).toThrow();
      }
    }
  });
});
