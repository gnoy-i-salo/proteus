import type {
  Evidence,
  ExperimentMemoryProtocol,
  PersonaMemoryProfile,
  PersonaRule,
  PersonaStyleProfile,
  PersonProfile,
  SkillDefinition,
  SourceSpan,
  StyleExemplar,
  StyleRule,
} from '@proteus-int/contracts';
import type { StalkerReport } from '@proteus-int/stalker';
import {
  asArray,
  asBoolean,
  asChoice,
  asInteger,
  asNullableText,
  asNullableTimestamp,
  asRecord,
  asStringArray,
  asText,
  exactKeys,
  field,
  invalid,
  optionalField,
} from './result-v4-validation';

const evidenceStatuses = ['supported', 'hypothesis', 'unknown', 'conflicting'] as const;
const verifications = ['model-reported', 'source-retrieved', 'unverified'] as const;
const communicationModes = ['dialogue', 'comment', 'formal'] as const;

export function decodeEvidence(value: unknown, path: string): Evidence {
  const record = asRecord(value, path);
  exactKeys(
    record,
    [
      'id',
      'sourceUrl',
      'kind',
      'summary',
      'status',
      'verification',
      'documentIds',
      'excerpt',
      'retrievedAt',
      'limitations',
    ],
    path,
  );
  const decoded: Evidence = {
    id: asText(field(record, 'id', path), `${path}.id`),
    sourceUrl: asText(field(record, 'sourceUrl', path), `${path}.sourceUrl`),
    kind: asChoice(field(record, 'kind', path), ['fact', 'hypothesis'], `${path}.kind`),
    summary: asText(field(record, 'summary', path), `${path}.summary`),
    status: asChoice(field(record, 'status', path), evidenceStatuses, `${path}.status`),
    verification: asChoice(
      field(record, 'verification', path),
      verifications,
      `${path}.verification`,
    ),
    excerpt: asNullableText(field(record, 'excerpt', path), `${path}.excerpt`),
    retrievedAt: asNullableTimestamp(field(record, 'retrievedAt', path), `${path}.retrievedAt`),
    limitations: asStringArray(field(record, 'limitations', path), `${path}.limitations`),
  };
  const documentIds = optionalField(record, 'documentIds');
  return documentIds === undefined
    ? decoded
    : { ...decoded, documentIds: asStringArray(documentIds, `${path}.documentIds`) };
}

export function decodePersonaRule(value: unknown, path: string): PersonaRule {
  const record = asRecord(value, path);
  exactKeys(record, ['id', 'instruction', 'evidenceIds', 'status', 'limitations'], path);
  return {
    id: asText(field(record, 'id', path), `${path}.id`),
    instruction: asText(field(record, 'instruction', path), `${path}.instruction`),
    evidenceIds: asStringArray(field(record, 'evidenceIds', path), `${path}.evidenceIds`),
    status: asChoice(field(record, 'status', path), evidenceStatuses, `${path}.status`),
    limitations: asStringArray(field(record, 'limitations', path), `${path}.limitations`),
  };
}

export function decodeStyleExemplar(value: unknown, path: string): StyleExemplar {
  const record = asRecord(value, path);
  exactKeys(
    record,
    ['id', 'documentId', 'mode', 'text', 'context', 'redactions', 'exportAllowed'],
    path,
  );
  const redactions = asArray(field(record, 'redactions', path), `${path}.redactions`).map(
    (item, index) => {
      const span = asRecord(item, `${path}.redactions[${index}]`);
      exactKeys(span, ['start', 'end'], `${path}.redactions[${index}]`);
      return {
        start: asInteger(span.start, `${path}.redactions[${index}].start`, 0),
        end: asInteger(span.end, `${path}.redactions[${index}].end`, 0),
      };
    },
  );
  return {
    id: asText(field(record, 'id', path), `${path}.id`),
    documentId: asText(field(record, 'documentId', path), `${path}.documentId`),
    mode: asChoice(field(record, 'mode', path), communicationModes, `${path}.mode`),
    text: asText(field(record, 'text', path), `${path}.text`),
    context: asText(field(record, 'context', path), `${path}.context`),
    redactions,
    exportAllowed: asBoolean(field(record, 'exportAllowed', path), `${path}.exportAllowed`),
  };
}

function decodeStyleRule(value: unknown, path: string): StyleRule {
  const record = asRecord(value, path);
  exactKeys(record, ['id', 'instruction', 'exemplarIds', 'status', 'limitations'], path);
  return {
    id: asText(field(record, 'id', path), `${path}.id`),
    instruction: asText(field(record, 'instruction', path), `${path}.instruction`),
    exemplarIds: asStringArray(field(record, 'exemplarIds', path), `${path}.exemplarIds`),
    status: asChoice(field(record, 'status', path), ['observed', 'hypothesis'], `${path}.status`),
    limitations: asStringArray(field(record, 'limitations', path), `${path}.limitations`),
  };
}

export function decodeStyleProfile(value: unknown, path: string): PersonaStyleProfile {
  const record = asRecord(value, path);
  exactKeys(
    record,
    [
      'version',
      'reasoningPatterns',
      'voiceRules',
      'formatRules',
      'contextModes',
      'styleExemplarIds',
      'limitations',
    ],
    path,
  );
  if (field(record, 'version', path) !== 2) invalid(`${path}.version`, 'expected version 2');
  const modes = asArray(field(record, 'contextModes', path), `${path}.contextModes`).map(
    (item, index) => {
      const mode = asRecord(item, `${path}.contextModes[${index}]`);
      exactKeys(mode, ['mode', 'status', 'limitations'], `${path}.contextModes[${index}]`);
      return {
        mode: asChoice(mode.mode, communicationModes, `${path}.contextModes[${index}].mode`),
        status: asChoice(
          mode.status,
          ['observed', 'insufficient'],
          `${path}.contextModes[${index}].status`,
        ),
        limitations: asStringArray(mode.limitations, `${path}.contextModes[${index}].limitations`),
      };
    },
  );
  return {
    version: 2,
    reasoningPatterns: asArray(
      field(record, 'reasoningPatterns', path),
      `${path}.reasoningPatterns`,
    ).map((item, index) => decodeStyleRule(item, `${path}.reasoningPatterns[${index}]`)),
    voiceRules: asArray(field(record, 'voiceRules', path), `${path}.voiceRules`).map(
      (item, index) => decodeStyleRule(item, `${path}.voiceRules[${index}]`),
    ),
    formatRules: asArray(field(record, 'formatRules', path), `${path}.formatRules`).map(
      (item, index) => decodeStyleRule(item, `${path}.formatRules[${index}]`),
    ),
    contextModes: modes,
    styleExemplarIds: asStringArray(
      field(record, 'styleExemplarIds', path),
      `${path}.styleExemplarIds`,
    ),
    limitations: asStringArray(field(record, 'limitations', path), `${path}.limitations`),
  };
}

function decodeSourceSpan(value: unknown, path: string): SourceSpan {
  const record = asRecord(value, path);
  exactKeys(record, ['documentId', 'revisionId', 'unit', 'start', 'end'], path);
  const start = asInteger(field(record, 'start', path), `${path}.start`, 0);
  return {
    documentId: asText(field(record, 'documentId', path), `${path}.documentId`),
    revisionId: asText(field(record, 'revisionId', path), `${path}.revisionId`),
    unit: asChoice(field(record, 'unit', path), ['utf8Bytes'], `${path}.unit`),
    start,
    end: asInteger(field(record, 'end', path), `${path}.end`, start),
  };
}

function decodeSkillDefinition(value: unknown, path: string): SkillDefinition {
  const record = asRecord(value, path);
  exactKeys(
    record,
    [
      'schemaVersion',
      'id',
      'trigger',
      'inputs',
      'steps',
      'outcomeCheck',
      'support',
      'memoryItemIds',
      'sourceSpans',
      'limitations',
    ],
    path,
  );
  if (field(record, 'schemaVersion', path) !== 1)
    invalid(`${path}.schemaVersion`, 'expected schemaVersion 1');
  return {
    schemaVersion: 1,
    id: asText(field(record, 'id', path), `${path}.id`),
    trigger: asText(field(record, 'trigger', path), `${path}.trigger`),
    inputs: asStringArray(field(record, 'inputs', path), `${path}.inputs`),
    steps: asStringArray(field(record, 'steps', path), `${path}.steps`),
    outcomeCheck: asText(field(record, 'outcomeCheck', path), `${path}.outcomeCheck`),
    support: asChoice(field(record, 'support', path), ['observed', 'proposed'], `${path}.support`),
    memoryItemIds: asStringArray(field(record, 'memoryItemIds', path), `${path}.memoryItemIds`),
    sourceSpans: asArray(field(record, 'sourceSpans', path), `${path}.sourceSpans`).map(
      (item, index) => decodeSourceSpan(item, `${path}.sourceSpans[${index}]`),
    ),
    limitations: asStringArray(field(record, 'limitations', path), `${path}.limitations`),
  };
}

function decodePersonaMemoryProfile(value: unknown, path: string): PersonaMemoryProfile {
  const record = asRecord(value, path);
  exactKeys(record, ['schemaVersion', 'manifestId', 'memorySnapshotId', 'skills'], path);
  if (field(record, 'schemaVersion', path) !== 1)
    invalid(`${path}.schemaVersion`, 'expected schemaVersion 1');
  return {
    schemaVersion: 1,
    manifestId: asText(field(record, 'manifestId', path), `${path}.manifestId`),
    memorySnapshotId: asText(field(record, 'memorySnapshotId', path), `${path}.memorySnapshotId`),
    skills: asArray(field(record, 'skills', path), `${path}.skills`).map((item, index) =>
      decodeSkillDefinition(item, `${path}.skills[${index}]`),
    ),
  };
}

export function decodeExperimentMemoryProtocol(
  value: unknown,
  path: string,
): ExperimentMemoryProtocol {
  const record = asRecord(value, path);
  exactKeys(
    record,
    ['schemaVersion', 'mode', 'participants', 'neutralKnowledgeCondition', 'limitations'],
    path,
  );
  if (field(record, 'schemaVersion', path) !== 1)
    invalid(`${path}.schemaVersion`, 'expected schemaVersion 1');
  const participants = asArray(field(record, 'participants', path), `${path}.participants`).map(
    (item, index) => {
      const participant = asRecord(item, `${path}.participants[${index}]`);
      exactKeys(participant, ['participantId', 'manifestId'], `${path}.participants[${index}]`);
      return {
        participantId: asChoice(
          participant.participantId,
          ['a', 'b', 'neutral'],
          `${path}.participants[${index}].participantId`,
        ),
        manifestId: asText(participant.manifestId, `${path}.participants[${index}].manifestId`),
      };
    },
  );
  return {
    schemaVersion: 1,
    mode: asChoice(field(record, 'mode', path), ['fixedContext', 'agenticRecall'], `${path}.mode`),
    participants,
    neutralKnowledgeCondition: asChoice(
      field(record, 'neutralKnowledgeCondition', path),
      ['matchedContext', 'matchedRecall', 'unmatched'],
      `${path}.neutralKnowledgeCondition`,
    ),
    limitations: asStringArray(field(record, 'limitations', path), `${path}.limitations`),
  };
}

export function decodePersonProfile(value: unknown, path: string): PersonProfile {
  const record = asRecord(value, path);
  exactKeys(
    record,
    [
      'id',
      'label',
      'palette',
      'archetype',
      'skills',
      'rules',
      'taskSkills',
      'sourceStatus',
      'sourceContext',
      'memory',
      'styleProfile',
      'styleExemplars',
    ],
    path,
  );
  const decoded: PersonProfile = {
    id: asText(field(record, 'id', path), `${path}.id`),
    label: asText(field(record, 'label', path), `${path}.label`),
    palette: asChoice(field(record, 'palette', path), ['turquoise', 'violet'], `${path}.palette`),
    archetype: asText(field(record, 'archetype', path), `${path}.archetype`),
    skills: asStringArray(field(record, 'skills', path), `${path}.skills`),
    rules: asArray(field(record, 'rules', path), `${path}.rules`).map((item, index) =>
      decodePersonaRule(item, `${path}.rules[${index}]`),
    ),
    taskSkills: asArray(field(record, 'taskSkills', path), `${path}.taskSkills`).map(
      (item, index) => decodePersonaRule(item, `${path}.taskSkills[${index}]`),
    ),
    sourceStatus: asChoice(
      field(record, 'sourceStatus', path),
      ['evidence-informed', 'synthetic', 'neutral'],
      `${path}.sourceStatus`,
    ),
  };
  const sourceContext = optionalField(record, 'sourceContext');
  const memory = optionalField(record, 'memory');
  const styleProfile = optionalField(record, 'styleProfile');
  const styleExemplars = optionalField(record, 'styleExemplars');
  return {
    ...decoded,
    ...(sourceContext !== undefined
      ? { sourceContext: asBoolean(sourceContext, `${path}.sourceContext`) }
      : {}),
    ...(memory !== undefined
      ? { memory: decodePersonaMemoryProfile(memory, `${path}.memory`) }
      : {}),
    ...(styleProfile !== undefined
      ? { styleProfile: decodeStyleProfile(styleProfile, `${path}.styleProfile`) }
      : {}),
    ...(styleExemplars !== undefined
      ? {
          styleExemplars: asArray(styleExemplars, `${path}.styleExemplars`).map((item, index) =>
            decodeStyleExemplar(item, `${path}.styleExemplars[${index}]`),
          ),
        }
      : {}),
  };
}

export function decodeStalkerReport(value: unknown, path: string): StalkerReport {
  const record = asRecord(value, path);
  exactKeys(
    record,
    [
      'evidence',
      'character',
      'thinkingStyle',
      'activity',
      'behaviorSummary',
      'styleExemplars',
      'sourceCoverage',
    ],
    path,
  );
  const decoded: StalkerReport = {
    evidence: asArray(field(record, 'evidence', path), `${path}.evidence`).map((item, index) =>
      decodeEvidence(item, `${path}.evidence[${index}]`),
    ),
    character: asText(field(record, 'character', path), `${path}.character`),
    thinkingStyle: asText(field(record, 'thinkingStyle', path), `${path}.thinkingStyle`),
    activity: asText(field(record, 'activity', path), `${path}.activity`),
    behaviorSummary: asText(field(record, 'behaviorSummary', path), `${path}.behaviorSummary`),
  };
  const styleExemplars = optionalField(record, 'styleExemplars');
  const sourceCoverage = optionalField(record, 'sourceCoverage');
  return {
    ...decoded,
    ...(styleExemplars !== undefined
      ? {
          styleExemplars: asArray(styleExemplars, `${path}.styleExemplars`).map((item, index) =>
            decodeStyleExemplar(item, `${path}.styleExemplars[${index}]`),
          ),
        }
      : {}),
    ...(sourceCoverage !== undefined
      ? {
          sourceCoverage: asArray(sourceCoverage, `${path}.sourceCoverage`).map((item, index) => {
            const coveragePath = `${path}.sourceCoverage[${index}]`;
            const coverage = asRecord(item, coveragePath);
            exactKeys(
              coverage,
              ['snapshotId', 'coverage', 'analyzedDocuments', 'limitations'],
              coveragePath,
            );
            const details = asRecord(coverage.coverage, `${coveragePath}.coverage`);
            exactKeys(
              details,
              [
                'documents',
                'totalDocuments',
                'bytes',
                'complete',
                'truncated',
                'notAnalyzed',
                'unavailable',
              ],
              `${coveragePath}.coverage`,
            );
            return {
              snapshotId: asText(
                field(coverage, 'snapshotId', coveragePath),
                `${coveragePath}.snapshotId`,
              ),
              coverage: {
                documents: asInteger(
                  field(details, 'documents', `${coveragePath}.coverage`),
                  `${coveragePath}.coverage.documents`,
                  0,
                ),
                totalDocuments:
                  details.totalDocuments === null
                    ? null
                    : asInteger(
                        field(details, 'totalDocuments', `${coveragePath}.coverage`),
                        `${coveragePath}.coverage.totalDocuments`,
                        0,
                      ),
                bytes: asInteger(
                  field(details, 'bytes', `${coveragePath}.coverage`),
                  `${coveragePath}.coverage.bytes`,
                  0,
                ),
                complete: asBoolean(
                  field(details, 'complete', `${coveragePath}.coverage`),
                  `${coveragePath}.coverage.complete`,
                ),
                truncated: asBoolean(
                  field(details, 'truncated', `${coveragePath}.coverage`),
                  `${coveragePath}.coverage.truncated`,
                ),
                notAnalyzed: asInteger(
                  field(details, 'notAnalyzed', `${coveragePath}.coverage`),
                  `${coveragePath}.coverage.notAnalyzed`,
                  0,
                ),
                unavailable: asStringArray(
                  field(details, 'unavailable', `${coveragePath}.coverage`),
                  `${coveragePath}.coverage.unavailable`,
                ),
              },
              analyzedDocuments: asInteger(
                field(coverage, 'analyzedDocuments', coveragePath),
                `${coveragePath}.analyzedDocuments`,
                0,
              ),
              limitations: asStringArray(
                field(coverage, 'limitations', coveragePath),
                `${coveragePath}.limitations`,
              ),
            };
          }),
        }
      : {}),
  };
}
