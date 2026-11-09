import { describe, expect, it } from 'vitest';
import { decodeFabricContext } from './fabric-context-codec';
import {
  fabricContextRequest,
  fabricEvidence,
  fabricLease,
  fabricRequest,
  fabricSession,
} from './testing/fabric-fixtures';

function decode(value: unknown) {
  return decodeFabricContext(
    value,
    fabricSession,
    fabricContextRequest,
    fabricRequest.modelBudget,
    fabricRequest.recallBudget,
  );
}
describe('Fabric native context reader', () => {
  it('preserves exact native input, selected memory, source attribution and receipts', () => {
    const input = fabricLease();
    const before = structuredClone(input);
    expect(decode(input)).toEqual(input);
    expect(input).toEqual(before);
  });
  it.each([
    { purpose: 'research' },
    { singleUse: false },
    { personaId: 'foreign' },
    { corpusSnapshotId: 'foreign' },
    { policyVersion: 'foreign' },
    { manifestId: 'manifest:1' },
    { expiresAt: '2026-11-09T04:01:00.000Z' },
    { inputBytes: 1 },
    { inputSha256: 'G'.repeat(64) },
    { contextSha256: 'bad' },
    { receiptIds: ['foreign'] },
    { sources: [] },
    { items: [] },
    { unknown: true },
    { formatAttempt: 2 },
    { systemPrompt: 'altered' },
  ])('rejects mismatched output: %j', (patch) => {
    expect(() => decode({ ...fabricLease(), ...patch })).toThrow();
  });
  it('requires exact framing and agreement between displayed and dispatched memory', () => {
    const value = fabricLease();
    for (const task of [
      fabricContextRequest.task,
      fabricContextRequest.task + '\nOther context',
      value.task + 'trailing',
    ]) {
      const inputBytes = new TextEncoder().encode(
        JSON.stringify([value.systemPrompt, task]),
      ).length;
      expect(() =>
        decode({ ...value, task, inputBytes, usage: { modelCalls: 1, inputBytes } }),
      ).toThrow();
    }
    const item = value.items[0]!;
    expect(() => decode({ ...value, items: [{ ...item, text: 'substituted' }] })).toThrow();
    const source = value.sources[0]!;
    expect(() =>
      decode({
        ...value,
        sources: [{ ...source, document: { ...source.document, authorLabel: 'other' } }],
      }),
    ).toThrow();
  });
  it('binds native receipts, selected item order and cumulative budgets', () => {
    const value = fabricLease();
    const evidence = fabricEvidence();
    evidence.retrievals[0]!.receipt.indexGeneration++;
    expect(() => decode(fabricLease(evidence))).toThrow();
    expect(() =>
      decodeFabricContext(
        value,
        fabricSession,
        { ...fabricContextRequest, itemIds: ['foreign'] },
        fabricRequest.modelBudget,
        fabricRequest.recallBudget,
      ),
    ).toThrow();
    expect(() =>
      decode({ ...value, usage: { modelCalls: 4, inputBytes: value.inputBytes } }),
    ).toThrow();
    expect(() =>
      decodeFabricContext(value, fabricSession, fabricContextRequest, fabricRequest.modelBudget, {
        ...fabricRequest.recallBudget,
        maxSourceBytes: 1,
      }),
    ).toThrow();
  });
  it('rejects unsupported attribution, false observed assertions and broken UTF8 spans', () => {
    for (const patch of [
      { snapshotId: 'foreign' },
      { policyVersion: 'foreign' },
      { episodeIds: ['foreign'] },
      { kind: 'reportedAssertion' as const },
      { text: 'not exact' },
      { sourceSpans: [{ ...fabricEvidence().items[0]!.sourceSpans[0]!, start: 3 }] },
    ]) {
      const evidence = fabricEvidence();
      evidence.items[0] = { ...evidence.items[0]!, ...patch };
      expect(() => decode(fabricLease(evidence))).toThrow();
    }
    const evidence = fabricEvidence();
    evidence.retrievals[0]!.sources[0] = {
      ...evidence.retrievals[0]!.sources[0]!,
      canSupportOwnWords: false,
    };
    expect(() => decode(fabricLease(evidence))).toThrow();
  });
  it('accepts empty published memory and format repair without fabricating evidence', () => {
    const evidence = fabricEvidence();
    evidence.items = [];
    evidence.retrievals = [];
    const value = { ...fabricLease(evidence), formatAttempt: 2 };
    expect(
      decodeFabricContext(
        value,
        fabricSession,
        { ...fabricContextRequest, itemIds: [], formatAttempt: 2 },
        fabricRequest.modelBudget,
        fabricRequest.recallBudget,
      ),
    ).toEqual(value);
  });
});
