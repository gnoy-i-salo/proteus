import type {
  EmbeddingFingerprint,
  PersonaManifest,
  SkillDefinition,
  SkillDefinitionDraft,
} from './memory';
import { choice, fields, integer } from './corpus-wire-validation';
import {
  memoryId,
  memoryIds,
  memoryList,
  memorySpans,
  memoryText,
  persistedMemoryTimestamp,
} from './memory-wire-validation';
import { decodeModelCallBudget, decodeRecallBudget } from './research-memory-codec';

function nonblank(value: unknown, maximum: number): string {
  const result = memoryText(value, maximum);
  if (!result.trim()) throw new Error('Пустое поле памяти.');
  return result;
}
const skillFields = [
  'trigger',
  'inputs',
  'steps',
  'outcomeCheck',
  'support',
  'memoryItemIds',
  'sourceSpans',
  'limitations',
];
function skillBody(input: Record<string, unknown>): Omit<SkillDefinition, 'id' | 'schemaVersion'> {
  const support = choice(input.support, ['observed', 'proposed']);
  const limitations = memoryList(input.limitations, 16, support === 'proposed' ? 1 : 0).map(
    (value) => nonblank(value, 2048),
  );
  return {
    trigger: nonblank(input.trigger, 4096),
    inputs: memoryList(input.inputs, 16).map((value) => nonblank(value, 2048)),
    steps: memoryList(input.steps, 16, 1).map((value) => nonblank(value, 2048)),
    outcomeCheck: nonblank(input.outcomeCheck, 4096),
    support,
    memoryItemIds: memoryIds(memoryList(input.memoryItemIds, 16, 1)),
    sourceSpans: memorySpans(input.sourceSpans),
    limitations,
  };
}
/** Shape only: source truth and observed skill support remain native-owned. */
export function decodeSkillDefinition(value: unknown): SkillDefinition {
  const input = fields(value, ['schemaVersion', 'id', ...skillFields]);
  if (input.schemaVersion !== 1) throw new Error('Неизвестная версия навыка.');
  return { schemaVersion: 1, id: memoryId(input.id), ...skillBody(input) };
}
export function decodeSkillDraft(value: unknown): SkillDefinitionDraft {
  const input = fields(value, ['draftId', ...skillFields]);
  return { draftId: memoryId(input.draftId), ...skillBody(input) };
}
export function decodeManifestSettings(value: unknown): PersonaManifest['modelSettings'] {
  const settings = memoryList(value, 4, 4).map(
    (value): PersonaManifest['modelSettings'][number] => {
      const input = fields(value, ['component', 'provider', 'requestedModel', 'requestedEffort']);
      return {
        component: choice(input.component, [
          'stalker',
          'person-fabric',
          'person',
          'brain-analyzator',
        ]),
        provider: choice(input.provider, ['codex', 'kimi']),
        requestedModel: input.requestedModel === null ? null : nonblank(input.requestedModel, 120),
        requestedEffort: choice(input.requestedEffort, ['low', 'medium', 'high']),
      };
    },
  );
  if (new Set(settings.map((entry) => entry.component)).size !== settings.length)
    throw new Error('Повторный компонент модели манифеста.');
  return settings;
}
function fingerprint(value: unknown): EmbeddingFingerprint | null {
  if (value === null) return null;
  const input = fields(value, [
    'modelId',
    'modelRevision',
    'modelSha256',
    'tokenizerSha256',
    'dimensions',
    'pooling',
    'normalization',
    'queryTemplate',
    'passageTemplate',
    'chunkingVersion',
  ]);
  const hash = (value: unknown): string => {
    if (typeof value !== 'string' || !/^[a-f0-9]{64}$/u.test(value))
      throw new Error('Неверный отпечаток модели.');
    return value;
  };
  const template = (value: unknown): string => {
    const result = memoryText(value, 4096);
    if (!result.length) throw new Error('Пустой шаблон embedding.');
    return result;
  };
  const dimensions = integer(input.dimensions, 1);
  if (dimensions > 4096) throw new Error('Неверная размерность embedding.');
  return {
    modelId: memoryId(input.modelId),
    modelRevision: memoryId(input.modelRevision),
    modelSha256: hash(input.modelSha256),
    tokenizerSha256: hash(input.tokenizerSha256),
    dimensions,
    pooling: choice(input.pooling, ['mean', 'cls']),
    normalization: choice(input.normalization, ['l2', 'none']),
    queryTemplate: template(input.queryTemplate),
    passageTemplate: template(input.passageTemplate),
    chunkingVersion: memoryId(input.chunkingVersion),
  };
}
export function decodeManifestReferences(value: unknown): PersonaManifest['sourceRefs'] {
  const references = memoryList(value, 10000).map((value) => {
    const input = fields(value, ['documentId', 'revisionId']);
    return { documentId: memoryId(input.documentId), revisionId: memoryId(input.revisionId) };
  });
  if (new Set(references.map((reference) => JSON.stringify(reference))).size !== references.length)
    throw new Error('Повторный источник манифеста.');
  return references;
}
/** Persisted V1 reader; reading a manifest never grants access to its sources. */
export function decodePersonaManifest(value: unknown): PersonaManifest {
  const input = fields(value, [
    'schemaVersion',
    'id',
    'personaId',
    'profileVersion',
    'skillIds',
    'exemplarIds',
    'corpusSnapshotId',
    'memorySnapshotId',
    'sourceRefs',
    'splitVersion',
    'preparationVersion',
    'retrievalVersion',
    'promptVersion',
    'embeddingFingerprint',
    'mode',
    'recallBudget',
    'modelCallBudget',
    'modelSettings',
    'createdAt',
  ]);
  if (input.schemaVersion !== 1) throw new Error('Неизвестная версия манифеста.');
  return {
    schemaVersion: 1,
    id: memoryId(input.id),
    personaId: memoryId(input.personaId),
    profileVersion: memoryId(input.profileVersion),
    skillIds: memoryIds(memoryList(input.skillIds, 64)),
    exemplarIds: memoryIds(memoryList(input.exemplarIds, 100)),
    corpusSnapshotId: memoryId(input.corpusSnapshotId),
    memorySnapshotId: memoryId(input.memorySnapshotId),
    sourceRefs: decodeManifestReferences(input.sourceRefs),
    splitVersion: memoryId(input.splitVersion),
    preparationVersion: memoryId(input.preparationVersion),
    retrievalVersion: memoryId(input.retrievalVersion),
    promptVersion: memoryId(input.promptVersion),
    embeddingFingerprint: fingerprint(input.embeddingFingerprint),
    mode: choice(input.mode, ['fixedContext', 'agenticRecall']),
    recallBudget: decodeRecallBudget(input.recallBudget),
    modelCallBudget: decodeModelCallBudget(input.modelCallBudget),
    modelSettings: decodeManifestSettings(input.modelSettings),
    createdAt: persistedMemoryTimestamp(input.createdAt),
  };
}
