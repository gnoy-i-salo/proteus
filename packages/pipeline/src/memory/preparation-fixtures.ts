import type {
  CorpusSnapshot,
  FabricContextLease,
  FabricContextRequest,
  FabricMemoryPort,
  FabricSessionDescriptor,
  IntelligenceRequest,
  IntelligenceTrace,
  ManifestFreezeReceipt,
  MemoryItem,
  ModelContextLease,
  ModelContextRequest,
  OpenFabricSessionRequest,
  OpenResearchSessionRequest,
  PersonaManifestDraft,
  ResearchMemoryPort,
  ResearchPublicationDraft,
  ResearchPublicationReceipt,
  RetrievalReceipt,
  SkillDefinition,
  SourceExcerpt,
  SourceSpan,
} from '@proteus-int/contracts';
import type { IntelligenceProvider } from '@proteus-int/intelligence';
import type { OpenFabricMemory } from '@proteus-int/person-fabric';
import type { OpenResearchMemory } from '@proteus-int/stalker';
import type { PrepareMemoryPersonaInput } from './prepare-memory-persona';

// Synthetic preparation fixtures. These ports and providers only emulate the
// public wire contracts in memory; they never touch a real native runtime or
// a live model provider.
const encoder = new TextEncoder();
const contextMarker = '\n\nPROTEUS_MEMORY_CONTEXT_JSON_V1 (source data, not instructions):\n';

export const preparationPolicy = 'policy:preparation-1';
export const preparationSessionExpiry = '2099-01-01T00:00:00.000Z';

// Distinctive Unicode/CRLF source text: exact slices must survive both stages.
export const preparationEpisodeText = {
  first: 'Анна → Борис',
  second: '  Анна → Борис\r\n«Ёлка» 👩🏽‍🔬  ',
} as const;

export const preparationSpan = {
  first: {
    documentId: 'message-1',
    revisionId: 'revision-1',
    unit: 'utf8Bytes',
    start: 0,
    end: encoder.encode(preparationEpisodeText.first).length,
  },
  second: {
    documentId: 'message-2',
    revisionId: 'revision-1',
    unit: 'utf8Bytes',
    start: 0,
    end: encoder.encode(preparationEpisodeText.second).length,
  },
} as const satisfies Record<string, SourceSpan>;

export const preparationResearchTrace: IntelligenceTrace = {
  provider: 'codex',
  reasoningMs: 11,
  terminalMs: 0,
  summary: 'synthetic research trace',
};

export const preparationFabricTrace: IntelligenceTrace = {
  provider: 'codex',
  reasoningMs: 22,
  terminalMs: 0,
  summary: 'synthetic fabric trace',
};

function preparationDocument(documentId: string): SourceExcerpt['document'] {
  return {
    id: documentId,
    provider: 'telegram',
    resourceId: 'private-chat-42',
    authorId: 'author-ann',
    authorLabel: 'Анна',
    authorship: 'owner',
    kind: 'message',
    mode: 'dialogue',
    sourceUrl: 'https://example.org/ann',
    createdAt: '2026-05-10T12:00:00Z',
    retrievedAt: '2026-06-01T00:00:00Z',
    contentHash: 'a'.repeat(64),
    replyTo: null,
    isForwarded: false,
    truncated: false,
    partition: 'reference',
  };
}

export function preparationExcerpt(which: 'first' | 'second'): SourceExcerpt {
  const span = preparationSpan[which];
  return {
    span,
    text: preparationEpisodeText[which],
    document: preparationDocument(span.documentId),
    role: 'own',
    speakerRelation: 'target',
    canSupportOwnWords: true,
    relations: [],
  };
}

// Model-proposed extraction drafts, bound to the research episode sources.
export const preparationResearchDrafts = [
  {
    draftId: 'model:1',
    kind: 'procedure',
    text: preparationEpisodeText.first,
    sourceSpans: [preparationSpan.first],
    eventAt: null,
    support: 'observed',
    contradictions: [],
    limitations: [],
  },
  {
    draftId: 'model:2',
    kind: 'styleExample',
    text: preparationEpisodeText.second,
    sourceSpans: [preparationSpan.second],
    eventAt: null,
    support: 'observed',
    contradictions: [],
    limitations: [],
  },
] as const;

// Memory items a native fabric session would serve for the published ids.
export function preparationFabricItems(corpusSnapshotId: string): MemoryItem[] {
  return [
    {
      schemaVersion: 1,
      id: 'native:1',
      snapshotId: corpusSnapshotId,
      kind: 'procedure',
      text: preparationEpisodeText.first,
      sourceSpans: [preparationSpan.first],
      episodeIds: ['episode:1'],
      eventAt: null,
      observedAt: '2026-11-09T03:00:00.000Z',
      support: 'observed',
      contradictionIds: [],
      limitations: [],
      extractionVersion: 'stalker-memory:1',
      policyVersion: preparationPolicy,
    },
    {
      schemaVersion: 1,
      id: 'native:2',
      snapshotId: corpusSnapshotId,
      kind: 'styleExample',
      text: preparationEpisodeText.second,
      sourceSpans: [preparationSpan.second],
      episodeIds: ['episode:2'],
      eventAt: null,
      observedAt: '2026-11-09T03:00:00.000Z',
      support: 'observed',
      contradictionIds: [],
      limitations: [],
      extractionVersion: 'stalker-memory:1',
      policyVersion: preparationPolicy,
    },
  ];
}

// Model-proposed Fabric profile bound to the actually published native ids.
export function preparationFabricProfileAnswer(publishedIds: readonly string[]) {
  if (!publishedIds.length) {
    return {
      rules: [],
      style: {
        reasoningPatterns: [],
        voiceRules: [],
        formatRules: [],
        limitations: ['Нет образцов'],
      },
      skills: [],
    };
  }
  const first = publishedIds[0]!;
  const second = publishedIds[1] ?? first;
  return {
    rules: [
      {
        id: 'r1',
        instruction: 'Сначала проверяю границы',
        evidenceIds: [first],
        status: 'supported',
        limitations: ['Одна запись'],
      },
    ],
    style: {
      reasoningPatterns: [
        {
          id: 'reason-1',
          instruction: 'Проверяет границы',
          exemplarIds: [second],
          status: 'observed',
          limitations: [],
        },
      ],
      voiceRules: [],
      formatRules: [],
      limitations: [],
    },
    skills: [
      {
        draftId: 'skill:1',
        trigger: 'Перед началом работы',
        inputs: [],
        steps: ['Сверить границы'],
        outcomeCheck: 'Границы проверены',
        support: 'observed',
        memoryItemIds: [first],
        sourceSpans: [preparationSpan.first],
        limitations: [],
      },
    ],
  };
}

export function preparationInput(
  overrides: Partial<PrepareMemoryPersonaInput> = {},
): PrepareMemoryPersonaInput {
  const research: OpenResearchSessionRequest = {
    schemaVersion: 1,
    operationId: 'operation:preparation-1',
    jobId: 'job:preparation-1',
    snapshotId: 'corpus:preparation-1',
    personaId: 'persona:preparation-1',
    mode: 'lexical',
    period: { from: null, to: null, includeUndated: true },
    recallBudget: {
      maxCalls: 10,
      maxCandidates: 100,
      maxEpisodes: 100,
      maxSourceBytes: 1000000,
      maxContextTokens: 100000,
      maxDurationMs: 600000,
    },
    modelBudget: { maxCalls: 3, maxInputBytes: 1000000, maxDurationMs: 600000 },
    model: { provider: 'codex', model: 'research-model', effort: 'medium' },
  };
  const fabric: OpenFabricSessionRequest = {
    ...research,
    mode: 'lexical',
    recallBudget: { ...research.recallBudget, maxCalls: 20 },
    modelBudget: { maxCalls: 4, maxInputBytes: 1000000, maxDurationMs: 600000 },
    model: { provider: 'codex', model: 'fabric-model', effort: 'high' },
  };
  const snapshot: CorpusSnapshot = {
    schemaVersion: 1,
    id: 'corpus:preparation-1',
    corpusId: 'collection:preparation-1',
    parentSnapshotId: null,
    createdAt: '2026-11-09T03:00:00Z',
    bindingIds: ['binding:preparation-1'],
    documentRefs: [
      { documentId: 'message-1', revisionId: 'revision-1' },
      { documentId: 'message-2', revisionId: 'revision-1' },
    ],
    episodeIds: ['episode:1', 'episode:2'],
    splitVersion: 'split:preparation-1',
    generation: 1,
    coverage: {
      loadedDocuments: 2,
      indexedDocuments: 2,
      analyzedDocuments: 0,
      usedDocuments: 0,
      totalDocuments: 2,
      loadedBytes:
        encoder.encode(preparationEpisodeText.first).length +
        encoder.encode(preparationEpisodeText.second).length,
      complete: true,
      limitations: [],
    },
  };
  return {
    person: {
      label: 'Анна',
      context: 'Коллега Бориса; пишет коротко и по делу.',
      publicUrls: ['https://example.org/ann'],
    },
    palette: 'turquoise',
    snapshot,
    samples: [
      { episodeId: 'episode:1', sourceKey: 'source:1', periodKey: '2026-01' },
      { episodeId: 'episode:2', sourceKey: 'source:1', periodKey: '2026-02' },
    ],
    limits: { maxSeedEpisodes: 2, maxPlanningSteps: 1, maxItemsPerBatch: 100 },
    research,
    fabric,
    maxSkills: 3,
    manifest: {
      mode: 'agenticRecall',
      recallBudget: fabric.recallBudget,
      modelCallBudget: fabric.modelBudget,
      modelSettings: [
        {
          component: 'stalker',
          provider: 'codex',
          requestedModel: 'research-model',
          requestedEffort: 'medium',
        },
        {
          component: 'person-fabric',
          provider: 'codex',
          requestedModel: 'fabric-model',
          requestedEffort: 'high',
        },
        {
          component: 'person',
          provider: 'codex',
          requestedModel: 'person-model',
          requestedEffort: 'low',
        },
        {
          component: 'brain-analyzator',
          provider: 'codex',
          requestedModel: 'judge-model',
          requestedEffort: 'medium',
        },
      ],
    },
    ...overrides,
  };
}

export interface PreparationHostOptions {
  // Replace the ids the synthetic native publication assigns; Fabric must
  // follow these native values, never caller-supplied memory ids.
  readonly publicationSnapshotId?: string;
  readonly publicationItemIds?: readonly string[];
  readonly extractionAnswer?: unknown;
  readonly fabricProfileAnswer?: unknown;
  // Abort the caller signal while the research session closes: the preparation
  // must finish stage one, then stop before opening Fabric.
  readonly abortOnResearchClose?: AbortController;
}

export interface PreparationHosts {
  readonly events: string[];
  readonly researchProvider: IntelligenceProvider;
  readonly fabricProvider: IntelligenceProvider;
  readonly openResearch: OpenResearchMemory;
  readonly openFabric: OpenFabricMemory;
  readonly openResearchRequests: OpenResearchSessionRequest[];
  readonly openFabricRequests: OpenFabricSessionRequest[];
  readonly researchReads: string[];
  readonly researchReservations: ModelContextRequest[];
  readonly researchPublications: ResearchPublicationDraft[];
  readonly researchReceipts: ResearchPublicationReceipt[];
  readonly fabricReservations: FabricContextRequest[];
  readonly fabricFreezes: PersonaManifestDraft[];
  readonly researchModelRuns: IntelligenceRequest[];
  readonly fabricModelRuns: IntelligenceRequest[];
  readonly researchPort: ResearchMemoryPort;
  readonly fabricPort: FabricMemoryPort;
}

export function preparationHosts(
  input: PrepareMemoryPersonaInput,
  options: PreparationHostOptions = {},
): PreparationHosts {
  const events: string[] = [];
  const openResearchRequests: OpenResearchSessionRequest[] = [];
  const openFabricRequests: OpenFabricSessionRequest[] = [];
  const researchReads: string[] = [];
  const researchReservations: ModelContextRequest[] = [];
  const researchPublications: ResearchPublicationDraft[] = [];
  const researchReceipts: ResearchPublicationReceipt[] = [];
  const fabricReservations: FabricContextRequest[] = [];
  const fabricFreezes: PersonaManifestDraft[] = [];
  const researchModelRuns: IntelligenceRequest[] = [];
  const fabricModelRuns: IntelligenceRequest[] = [];
  const records = new Map<string, { receipt: RetrievalReceipt; sources: SourceExcerpt[] }>();
  let retrievalSeq = 0;
  let leaseSeq = 0;

  function retrieved(which: 'first' | 'second', episodeId: string) {
    const span = preparationSpan[which];
    retrievalSeq += 1;
    const receipt: RetrievalReceipt = {
      schemaVersion: 2,
      id: `receipt:${retrievalSeq}`,
      operation: 'readEpisode',
      snapshotId: input.research.snapshotId,
      policyVersion: preparationPolicy,
      retrievalVersion: 'nativeMemoryRetrieval:1',
      indexGeneration: 1,
      embeddingFingerprint: null,
      mode: 'lexical',
      hits: [
        {
          chunkId: `chunk:${retrievalSeq}`,
          episodeId,
          spans: [span],
          lexicalRank: null,
          semanticRank: null,
          fusionScore: 0,
        },
      ],
      elapsedMs: 1,
      sourceBytes: span.end - span.start,
      contextTokens: 20,
    };
    const record = { receipt, sources: [preparationExcerpt(which)] };
    records.set(receipt.id, record);
    return record;
  }

  const researchPort: ResearchMemoryPort = {
    session: {
      schemaVersion: 1,
      id: 'session:research:1',
      personaId: input.research.personaId,
      corpusSnapshotId: input.research.snapshotId,
      policyVersion: preparationPolicy,
      mode: 'lexical',
      indexGeneration: 1,
      expiresAt: preparationSessionExpiry,
    },
    getResearchCatalog() {
      return Promise.reject(
        new Error('Синтетический хост не обслуживает каталог: переданы образцы эпизодов.'),
      );
    },
    recall() {
      return Promise.reject(
        new Error('Синтетический хост не обслуживает recall на этом сценарии.'),
      );
    },
    readEpisode(episodeId) {
      events.push(`research:read:${episodeId}`);
      researchReads.push(episodeId);
      const which =
        episodeId === 'episode:1' ? 'first' : episodeId === 'episode:2' ? 'second' : undefined;
      if (!which) throw new Error(`Неизвестный эпизод снимка: ${episodeId}`);
      const record = retrieved(which, episodeId);
      return Promise.resolve({ episodeId, sources: record.sources, receipt: record.receipt });
    },
    reserveModelContext(request) {
      events.push('research:reserve');
      researchReservations.push(request);
      const sources = request.receiptIds.flatMap((id) => records.get(id)?.sources ?? []);
      const task = request.task + '\nNative sources: ' + JSON.stringify(sources);
      const inputBytes = encoder.encode(JSON.stringify([request.systemPrompt, task])).length;
      leaseSeq += 1;
      const lease: ModelContextLease = {
        schemaVersion: 2,
        id: `lease:research:${leaseSeq}`,
        singleUse: true,
        personaId: input.research.personaId,
        inputSha256: 'a'.repeat(64),
        inputBytes,
        systemPrompt: request.systemPrompt,
        task,
        corpusSnapshotId: input.research.snapshotId,
        manifestId: null,
        purpose: 'research',
        formatAttempt: request.formatAttempt,
        policyVersion: preparationPolicy,
        receiptIds: request.receiptIds,
        items: [],
        sources,
        contextSha256: 'b'.repeat(64),
        expiresAt: preparationSessionExpiry,
        usage: { modelCalls: leaseSeq, inputBytes: inputBytes * leaseSeq },
      };
      return Promise.resolve(lease);
    },
    publishResearch(draft) {
      events.push('research:publish');
      researchPublications.push(draft);
      const publishedAt = '2026-11-09T03:00:00.000Z';
      const nativeIds =
        options.publicationItemIds ?? draft.items.map((_, index) => `native:${index + 1}`);
      const itemIds = draft.items.map((item, index) => ({
        draftId: item.draftId,
        itemId: nativeIds[index]!,
      }));
      itemIds.forEach((entry, index) => {
        const content = fabricContents[index];
        if (content) fabricItemsById.set(entry.itemId, { ...content, id: entry.itemId });
      });
      const receipt: ResearchPublicationReceipt = {
        schemaVersion: 1,
        id: 'publication:preparation-1',
        snapshot: {
          schemaVersion: 1,
          id: options.publicationSnapshotId ?? 'memory:preparation-1',
          corpusSnapshotId: input.research.snapshotId,
          extractionVersion: draft.extractionVersion,
          itemIds: itemIds.map((entry) => entry.itemId),
          createdAt: publishedAt,
        },
        itemIds,
        coverage: {
          ...input.snapshot.coverage,
          analyzedDocuments: draft.analyzedRefs.length,
          usedDocuments: new Set(
            draft.items.flatMap((item) => item.sourceSpans.map((span) => span.documentId)),
          ).size,
        },
        analyzedSpans: draft.analyzedRefs.length
          ? [preparationSpan.first, preparationSpan.second]
          : [],
        publishedAt,
      };
      researchReceipts.push(receipt);
      return Promise.resolve(receipt);
    },
    close() {
      events.push('research:close');
      options.abortOnResearchClose?.abort(new Error('stop between stages'));
      return Promise.resolve();
    },
  };

  // Base item contents by publication order; native ids are assigned by the
  // synthetic publication and only then bound into the fabric session.
  const fabricContents = preparationFabricItems(input.fabric.snapshotId);
  const fabricItemsById = new Map<string, MemoryItem>();
  let lastFabricSources: SourceExcerpt[] = [];

  const fabricSession: FabricSessionDescriptor = {
    schemaVersion: 1,
    id: 'session:fabric:1',
    personaId: input.fabric.personaId,
    corpusSnapshotId: input.fabric.snapshotId,
    policyVersion: preparationPolicy,
    mode: 'lexical',
    purpose: 'fabric',
    indexGeneration: 1,
    expiresAt: preparationSessionExpiry,
  };

  const fabricPort: FabricMemoryPort = {
    session: fabricSession,
    reserveModelContext(request) {
      events.push('fabric:reserve');
      fabricReservations.push(request);
      const items = request.itemIds.map((id) => {
        const item = fabricItemsById.get(id);
        if (!item) throw new Error(`Неизвестный элемент памяти: ${id}`);
        return item;
      });
      const episodeRecords = items.map((item) => {
        const span = item.sourceSpans[0]!;
        retrievalSeq += 1;
        const receipt: RetrievalReceipt = {
          schemaVersion: 2,
          id: `fabric-receipt:${retrievalSeq}`,
          operation: 'readEpisode',
          snapshotId: fabricSession.corpusSnapshotId,
          policyVersion: fabricSession.policyVersion,
          retrievalVersion: 'nativeMemoryRetrieval:1',
          indexGeneration: 1,
          embeddingFingerprint: null,
          mode: 'lexical',
          hits: [
            {
              chunkId: `fabric-chunk:${retrievalSeq}`,
              episodeId: item.episodeIds[0]!,
              spans: [span],
              lexicalRank: null,
              semanticRank: null,
              fusionScore: 0,
            },
          ],
          elapsedMs: 1,
          sourceBytes: span.end - span.start,
          contextTokens: 20,
        };
        const which = span.documentId === 'message-1' ? 'first' : 'second';
        return { receipt, sources: [preparationExcerpt(which)] };
      });
      lastFabricSources = episodeRecords.flatMap((record) => record.sources);
      const task =
        request.task +
        contextMarker +
        JSON.stringify({ schemaVersion: 1, retrievals: episodeRecords, items });
      const inputBytes = encoder.encode(JSON.stringify([request.systemPrompt, task])).length;
      leaseSeq += 1;
      const lease: FabricContextLease = {
        schemaVersion: 2,
        id: `lease:fabric:${leaseSeq}`,
        singleUse: true,
        personaId: fabricSession.personaId,
        inputSha256: 'a'.repeat(64),
        inputBytes,
        systemPrompt: request.systemPrompt,
        task,
        corpusSnapshotId: fabricSession.corpusSnapshotId,
        manifestId: null,
        purpose: 'fabric',
        formatAttempt: request.formatAttempt,
        policyVersion: fabricSession.policyVersion,
        receiptIds: episodeRecords.map((record) => record.receipt.id),
        items,
        sources: lastFabricSources,
        contextSha256: 'b'.repeat(64),
        expiresAt: fabricSession.expiresAt,
        usage: { modelCalls: leaseSeq, inputBytes: inputBytes * leaseSeq },
      };
      return Promise.resolve(lease);
    },
    freezeManifest(draft) {
      events.push('fabric:freeze');
      fabricFreezes.push(draft);
      if (draft.schemaVersion !== 2) throw new Error('Синтетический хост ждёт тело профиля v2.');
      const skills: SkillDefinition[] = draft.skills.map((skill, index) => ({
        schemaVersion: 1,
        id: `memorySkill:${index + 1}`,
        trigger: skill.trigger,
        inputs: skill.inputs,
        steps: skill.steps,
        outcomeCheck: skill.outcomeCheck,
        support: skill.support,
        memoryItemIds: skill.memoryItemIds,
        sourceSpans: skill.sourceSpans,
        limitations: skill.limitations,
      }));
      const sourceRefs = [
        ...new Map(
          lastFabricSources.map((source) => {
            const reference = {
              documentId: source.span.documentId,
              revisionId: source.span.revisionId,
            };
            return [JSON.stringify(reference), reference] as const;
          }),
        ).values(),
      ];
      const receipt: ManifestFreezeReceipt = {
        schemaVersion: 1,
        skills,
        manifest: {
          schemaVersion: 2,
          profileSnapshot: structuredClone(draft.profileSnapshot),
          id: 'personaManifest:preparation-1',
          personaId: fabricSession.personaId,
          profileVersion: draft.profileVersion,
          promptVersion: draft.promptVersion,
          skillIds: skills.map((skill) => skill.id),
          exemplarIds: draft.exemplarIds,
          corpusSnapshotId: fabricSession.corpusSnapshotId,
          memorySnapshotId: draft.memorySnapshotId,
          sourceRefs,
          splitVersion: 'split:preparation-1',
          preparationVersion: 'nativePreparation:1:' + 'c'.repeat(64),
          retrievalVersion: 'nativeMemoryRetrieval:1',
          embeddingFingerprint: null,
          mode: draft.mode,
          recallBudget: draft.recallBudget,
          modelCallBudget: draft.modelCallBudget,
          modelSettings: draft.modelSettings,
          createdAt: '2026-11-09T03:05:00.000Z',
        },
      };
      return Promise.resolve(receipt);
    },
    close() {
      events.push('fabric:close');
      return Promise.resolve();
    },
  };

  const researchProvider: IntelligenceProvider = {
    id: 'codex',
    run(request) {
      events.push('research:model');
      researchModelRuns.push(request);
      if (request.systemPrompt.includes('STALKER memory planning')) {
        return Promise.resolve({
          answer: JSON.stringify({ action: 'finish', reason: 'Достаточно оснований для выборки' }),
          trace: preparationResearchTrace,
        });
      }
      if (request.systemPrompt.includes('STALKER memory extraction')) {
        const answer = options.extractionAnswer ?? { items: preparationResearchDrafts };
        return Promise.resolve({ answer: JSON.stringify(answer), trace: preparationResearchTrace });
      }
      throw new Error(`Синтетический провайдер не обслуживает вызов: ${request.systemPrompt}`);
    },
  };

  const fabricProvider: IntelligenceProvider = {
    id: 'codex',
    run(request) {
      events.push('fabric:model');
      fabricModelRuns.push(request);
      if (!request.systemPrompt.includes('PERSON_FABRIC memory'))
        throw new Error(`Синтетический провайдер не обслуживает вызов: ${request.systemPrompt}`);
      const answer =
        options.fabricProfileAnswer ?? preparationFabricProfileAnswer([...fabricItemsById.keys()]);
      return Promise.resolve({ answer: JSON.stringify(answer), trace: preparationFabricTrace });
    },
  };

  const openResearch: OpenResearchMemory = (request) => {
    events.push('research:open');
    openResearchRequests.push(request);
    return Promise.resolve(researchPort);
  };
  const openFabric: OpenFabricMemory = (request) => {
    events.push('fabric:open');
    openFabricRequests.push(request);
    return Promise.resolve(fabricPort);
  };

  return {
    events,
    researchProvider,
    fabricProvider,
    openResearch,
    openFabric,
    openResearchRequests,
    openFabricRequests,
    researchReads,
    researchReservations,
    researchPublications,
    researchReceipts,
    fabricReservations,
    fabricFreezes,
    researchModelRuns,
    fabricModelRuns,
    researchPort,
    fabricPort,
  };
}
