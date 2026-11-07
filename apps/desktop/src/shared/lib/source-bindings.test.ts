import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { SourceBindingRequest, SourceAction } from '@proteus-int/contracts';
import { createSourceBinding, sourceBindingAction } from './source-bindings';
import fixture from './fixtures/source-binding-wire.json';

const native = vi.hoisted(() => ({ desktop: true, invoke: vi.fn() }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: native.invoke }));
vi.mock('./window-controls', () => ({ isDesktop: () => native.desktop }));
const request = fixture.createRequest as SourceBindingRequest;
const action = fixture.action as SourceAction;
const projection = fixture.result.documents[0]!;
beforeEach(() => {
  vi.clearAllMocks();
  native.desktop = true;
});

describe('native source bindings', () => {
  it('uses the same canonical native fixture for creation and exact context observations', async () => {
    native.invoke.mockResolvedValueOnce(fixture.created).mockResolvedValueOnce(fixture.result);
    expect(await createSourceBinding(request)).toEqual(fixture.created);
    expect(native.invoke).toHaveBeenNthCalledWith(1, 'create_source_binding', { request });
    const result = await sourceBindingAction(fixture.created.binding.id, action);
    expect(native.invoke).toHaveBeenNthCalledWith(2, 'source_binding_action', {
      bindingId: fixture.created.binding.id,
      action,
    });
    expect(result).toEqual(fixture.result);
    expect(result.documents[0]?.document.text).toBe(projection.document.text);
    expect(result.documents[0]?.canSupportOwnWords).toBe(false);
  });

  it.each([
    { personaId: '' },
    { personaId: '   ' },
    { personaId: 'я'.repeat(65) },
    { personaId: 'bad\nID' },
    { role: 'invented' },
    { allowedContextAuthorIds: ['target:1'] },
    { allowedContextAuthorIds: ['peer:2', 'peer:2'] },
    { allowedContextAuthorIds: ['\u0000'] },
    { allowedContextAuthorIds: ['я'.repeat(65)] },
    { allowedContextAuthorIds: Array.from({ length: 129 }, (_, i) => `peer${i}`) },
    { allowUnknownContext: 'yes' },
    { includeUndated: null },
    { 'persona-id': 'old' },
    { source: { ...request.source, authorId: null } },
    { source: { ...request.source, authorId: undefined } },
  ])('rejects malformed or expanded binding requests before IPC (%#)', async (changes) => {
    native.invoke.mockResolvedValue(fixture.created);
    await expect(
      createSourceBinding({ ...request, ...changes } as unknown as SourceBindingRequest),
    ).rejects.toThrow();
    expect(native.invoke).not.toHaveBeenCalled();
  });

  it('accepts native resource canonicalization without resolving identifiers in frontend', async () => {
    const selected = {
      ...request,
      source: {
        ...request.source,
        provider: 'github' as const,
        resourceId: 'https://github.com/owner/repo',
      },
    };
    const created = {
      binding: { ...fixture.created.binding, provider: 'github', resourceId: 'owner/repo' },
      grant: { ...fixture.created.grant, provider: 'github', resourceId: 'owner/repo' },
    };
    native.invoke.mockResolvedValue(created);
    expect(await createSourceBinding(selected)).toEqual(created);
    expect(native.invoke).toHaveBeenCalledExactlyOnceWith('create_source_binding', {
      request: selected,
    });
  });

  it('keeps legacy Instagram display-name metadata while accepting native unknown authorship', async () => {
    const document = {
      ...projection,
      speakerRelation: 'unknown',
      document: {
        ...projection.document,
        provider: 'instagram',
        authorId: '  Анна Иванова  ',
        authorLabel: 'Анна Иванова',
        authorship: 'unknown',
      },
    };
    native.invoke.mockResolvedValue({ ...fixture.result, documents: [document] });
    expect((await sourceBindingAction('binding-id', action)).documents).toEqual([document]);
    native.invoke.mockResolvedValue({
      ...fixture.result,
      documents: [{ ...projection, document: { ...projection.document, authorId: null } }],
    });
    await expect(sourceBindingAction('binding-id', action)).rejects.toThrow();
  });

  it.each([
    { ...fixture.created, extra: true },
    { ...fixture.created, binding: { ...fixture.created.binding, schemaVersion: 2 } },
    { ...fixture.created, binding: { ...fixture.created.binding, 'target-author-id': 'target:1' } },
    { ...fixture.created, binding: { ...fixture.created.binding, grantId: 'other' } },
    { ...fixture.created, binding: { ...fixture.created.binding, accountId: null } },
    { ...fixture.created, binding: { ...fixture.created.binding, personaId: 'other-person' } },
    { ...fixture.created, binding: { ...fixture.created.binding, targetAuthorId: 'other-author' } },
    {
      ...fixture.created,
      binding: { ...fixture.created.binding, allowedContextAuthorIds: ['foreign-author'] },
    },
    { ...fixture.created, grant: { ...fixture.created.grant, resourceId: 'foreign-resource' } },
  ])('rejects malformed or mismatched issued binding/grant responses (%#)', async (value) => {
    native.invoke.mockResolvedValue(value);
    await expect(createSourceBinding(request)).rejects.toThrow();
  });

  it.each([
    { action: 'search', query: 'я'.repeat(129), limit: 1 },
    { action: 'search', query: ' ', limit: 1 },
    { action: 'collect', cursor: null, limit: 21 },
    { action: 'read', documentIds: [] },
    { action: 'read', documentIds: ['same', 'same'] },
    { action: 'read', documentIds: ['x'], grantId: 'foreign' },
    { action: 'finish', reason: 'not a native action' },
  ])('rejects malformed actions before IPC (%#)', async (value) => {
    native.invoke.mockResolvedValue(fixture.result);
    await expect(sourceBindingAction('binding-id', value as SourceAction)).rejects.toThrow();
    expect(native.invoke).not.toHaveBeenCalled();
  });

  it.each([
    { ...projection, canSupportOwnWords: true },
    { ...projection, speakerRelation: 'invented' },
    { ...projection, revisionId: null },
    { ...projection, role: 'invented' },
    { ...projection, grantId: 'authority-must-not-leak' },
    { ...projection, document: { ...projection.document, partition: 'holdout' } },
    { ...projection, document: { ...projection.document, 'author-id': 'peer:2' } },
    {
      ...projection,
      relations: [
        {
          kind: 'reply',
          original: null,
          originalAuthor: { provider: null, id: null, label: null },
          span: { documentId: 'd', revisionId: 'r', unit: 'utf8Bytes', start: 4, end: 3 },
        },
      ],
    },
  ])('rejects malformed or misleading source projections (%#)', async (value) => {
    native.invoke.mockResolvedValue({ ...fixture.result, documents: [value] });
    await expect(sourceBindingAction('binding-id', action)).rejects.toThrow();
  });

  it('preserves target, unknown, forwarded and quoted authorship without upgrading source words', async () => {
    const own = {
      ...projection,
      speakerRelation: 'target',
      canSupportOwnWords: true,
      document: {
        ...projection.document,
        authorId: 'target:1',
        authorship: 'owner',
        text: '  Анна\r\n👩🏽‍🔬  ',
      },
    };
    const unknown = {
      ...projection,
      speakerRelation: 'unknown',
      document: {
        ...projection.document,
        authorId: null,
        authorship: 'unknown',
        authorLabel: null,
      },
    };
    const forward = {
      ...own,
      canSupportOwnWords: false,
      document: { ...own.document, isForwarded: true },
    };
    const quote = {
      ...own,
      canSupportOwnWords: false,
      relations: [
        {
          kind: 'quote',
          original: null,
          originalAuthor: { provider: 'vk', id: 'original', label: 'Борис\r\n' },
          span: {
            documentId: 'document:peer',
            revisionId: 'revision:peer',
            unit: 'utf8Bytes',
            start: 0,
            end: 2,
          },
        },
      ],
    };
    const documents = [own, unknown, forward, quote];
    native.invoke.mockResolvedValue({ ...fixture.result, documents });
    expect((await sourceBindingAction('binding-id', action)).documents).toEqual(documents);
    native.invoke.mockResolvedValue({
      ...fixture.result,
      documents: [{ ...forward, canSupportOwnWords: true }],
    });
    await expect(sourceBindingAction('binding-id', action)).rejects.toThrow();
    native.invoke.mockResolvedValue({
      ...fixture.result,
      documents: [{ ...quote, canSupportOwnWords: true }],
    });
    await expect(sourceBindingAction('binding-id', action)).rejects.toThrow();
  });

  it('does not invoke desktop commands in browser mode or after cancellation', async () => {
    native.desktop = false;
    await expect(createSourceBinding(request)).rejects.toThrow();
    await expect(sourceBindingAction('binding-id', action)).rejects.toThrow();
    native.desktop = true;
    const controller = new AbortController();
    controller.abort();
    await expect(createSourceBinding(request, controller.signal)).rejects.toThrow();
    await expect(sourceBindingAction('binding-id', action, controller.signal)).rejects.toThrow();
    expect(native.invoke).not.toHaveBeenCalled();
  });
  it('suppresses a late response after cancellation without claiming native cancellation', async () => {
    const controller = new AbortController();
    native.invoke.mockImplementation(async () => {
      controller.abort();
      return fixture.result;
    });
    await expect(sourceBindingAction('binding-id', action, controller.signal)).rejects.toThrow();
    expect(native.invoke).toHaveBeenCalledOnce();
  });
  it('propagates native rejection without fabricated data or a fallback source request', async () => {
    native.invoke.mockRejectedValue(new Error('permission revoked'));
    await expect(sourceBindingAction('binding-id', action)).rejects.toThrow('permission revoked');
    expect(native.invoke).toHaveBeenCalledOnce();
  });
});
