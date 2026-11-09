// Per-object decoders for the schema-owned input, report, profile, style and
// export-metadata shapes of the exact legacy series-result schemas. Every
// decoder validates its own exact key set and era optionals; unknown keys,
// mixed kebab/camel aliases and era-mismatched fields are rejected here.
import type {
  LegacySeriesDecodeContext,
  LegacySeriesEvidence,
  LegacySeriesExperimentInput,
  LegacySeriesExportMetadata,
  LegacySeriesPersonInput,
  LegacySeriesPersonProfile,
  LegacySeriesPersonRule,
  LegacySeriesRedactionSpan,
  LegacySeriesReport,
  LegacySeriesSourceCoverage,
  LegacySeriesSourceCoverageEntry,
  LegacySeriesStyleExemplar,
  LegacySeriesStyleProfile,
  LegacySeriesStyleRule,
} from './legacy-series-types';
import {
  artifactCountMax,
  artifactKeyMax,
  artifactValueMax,
  booleanOf,
  choiceOf,
  contextMax,
  idMax,
  invalid,
  labelMax,
  listMax,
  modelTextMax,
  nonBlankText,
  opaqueIdList,
  recordFields,
  rulesMax,
  safeIntegerAtLeast,
  taskMax,
  textList,
  textOf,
  timestampOrNull,
  urlListMax,
  urlTextMax,
} from './legacy-series-validation';

const evidenceIdPattern = /^e\d+$/u;

export function decodePersonInput(value: unknown, contextPath: string): LegacySeriesPersonInput {
  const source = recordFields(value, ['label', 'context', 'public-urls'], [], contextPath);
  if (!Array.isArray(source['public-urls']) || source['public-urls'].length > urlListMax)
    invalid(`${contextPath}.public-urls must be a list of at most ${urlListMax} URLs`);
  return {
    label: nonBlankText(source.label, labelMax, `${contextPath} 'label'`),
    context: textOf(source.context, contextMax, `${contextPath}.context`),
    publicUrls: (source['public-urls'] as unknown[]).map((item: unknown, index: number) =>
      textOf(item, urlTextMax, `${contextPath}.public-urls[${index}]`),
    ),
  };
}

export function decodeInput(value: unknown): LegacySeriesExperimentInput {
  const source = recordFields(value, ['task', 'people'], [], 'result.input');
  if (!Array.isArray(source.people) || source.people.length !== 2)
    invalid('result.input.people must list exactly two people');
  return {
    task: nonBlankText(source.task, taskMax, "input 'task'"),
    people: [
      decodePersonInput(source.people[0], 'result.input.people[0]'),
      decodePersonInput(source.people[1], 'result.input.people[1]'),
    ],
  };
}

export function decodeEvidence(
  value: unknown,
  context: LegacySeriesDecodeContext,
  contextPath: string,
): LegacySeriesEvidence {
  const optional = context.schemaVersion === 3 ? ['document-ids'] : [];
  const source = recordFields(
    value,
    [
      'id',
      'source-url',
      'kind',
      'summary',
      'status',
      'verification',
      'excerpt',
      'retrieved-at',
      'limitations',
    ],
    optional,
    contextPath,
  );
  const id = nonBlankText(source.id, idMax, `${contextPath}.id`);
  if (!evidenceIdPattern.test(id))
    invalid(`${contextPath}.id must be an opaque evidence id such as e1`);
  const verifications =
    context.schemaVersion === 3
      ? (['model-reported', 'unverified', 'source-retrieved'] as const)
      : (['model-reported', 'unverified'] as const);
  const evidence: LegacySeriesEvidence = {
    id,
    sourceUrl: textOf(source['source-url'], urlTextMax, `${contextPath}.source-url`),
    kind: choiceOf(source.kind, ['fact', 'hypothesis'] as const, `${contextPath}.kind`),
    summary: nonBlankText(source.summary, modelTextMax, `${contextPath}.summary`),
    status: choiceOf(
      source.status,
      ['supported', 'hypothesis', 'unknown', 'conflicting'] as const,
      `${contextPath}.status`,
    ),
    verification: choiceOf(source.verification, verifications, `${contextPath} 'verification'`),
    excerpt:
      source.excerpt === null
        ? null
        : textOf(source.excerpt, modelTextMax, `${contextPath}.excerpt`),
    retrievedAt: timestampOrNull(source['retrieved-at'], `${contextPath}.retrieved-at`),
    limitations: textList(source.limitations, `${contextPath}.limitations`),
  };
  if (context.schemaVersion === 3 && 'document-ids' in source) {
    Object.assign(evidence, {
      documentIds: opaqueIdList(source['document-ids'], `${contextPath}.document-ids`),
    });
  }
  return evidence;
}

export function decodeSourceCoverage(
  value: unknown,
  contextPath: string,
): LegacySeriesSourceCoverage {
  const source = recordFields(
    value,
    [
      'documents',
      'total-documents',
      'bytes',
      'complete',
      'truncated',
      'not-analyzed',
      'unavailable',
    ],
    [],
    contextPath,
  );
  return {
    documents: safeIntegerAtLeast(source.documents, 0, `${contextPath}.documents`),
    totalDocuments:
      source['total-documents'] === null
        ? null
        : safeIntegerAtLeast(source['total-documents'], 0, `${contextPath}.total-documents`),
    bytes: safeIntegerAtLeast(source.bytes, 0, `${contextPath}.bytes`),
    complete: booleanOf(source.complete, `${contextPath}.complete`),
    truncated: booleanOf(source.truncated, `${contextPath}.truncated`),
    notAnalyzed: safeIntegerAtLeast(source['not-analyzed'], 0, `${contextPath}.not-analyzed`),
    unavailable: textList(source.unavailable, `${contextPath}.unavailable`),
  };
}

export function decodeSourceCoverageEntry(
  value: unknown,
  contextPath: string,
): LegacySeriesSourceCoverageEntry {
  const source = recordFields(
    value,
    ['snapshot-id', 'coverage', 'analyzed-documents', 'limitations'],
    [],
    contextPath,
  );
  return {
    snapshotId: nonBlankText(source['snapshot-id'], idMax, `${contextPath}.snapshot-id`),
    coverage: decodeSourceCoverage(source.coverage, `${contextPath}.coverage`),
    analyzedDocuments: safeIntegerAtLeast(
      source['analyzed-documents'],
      0,
      `${contextPath}.analyzed-documents`,
    ),
    limitations: textList(source.limitations, `${contextPath}.limitations`),
  };
}

export function decodeStyleExemplar(
  value: unknown,
  contextPath: string,
): LegacySeriesStyleExemplar {
  const source = recordFields(
    value,
    ['id', 'document-id', 'mode', 'text', 'context', 'redactions', 'export-allowed'],
    [],
    contextPath,
  );
  if (!Array.isArray(source.redactions) || source.redactions.length > listMax)
    invalid(`${contextPath}.redactions must be a list of at most ${listMax} spans`);
  return {
    id: nonBlankText(source.id, idMax, `${contextPath}.id`),
    documentId: nonBlankText(source['document-id'], idMax, `${contextPath}.document-id`),
    mode: choiceOf(source.mode, ['dialogue', 'comment', 'formal'] as const, `${contextPath}.mode`),
    text: textOf(source.text, modelTextMax, `${contextPath}.text`),
    context: textOf(source.context, modelTextMax, `${contextPath}.context`),
    redactions: source.redactions.map((item: unknown, index: number): LegacySeriesRedactionSpan => {
      const span = recordFields(item, ['start', 'end'], [], `${contextPath}.redactions[${index}]`);
      const start = safeIntegerAtLeast(span.start, 0, `${contextPath}.redactions[${index}].start`);
      const end = safeIntegerAtLeast(span.end, 0, `${contextPath}.redactions[${index}].end`);
      if (start > end) invalid(`${contextPath}.redactions[${index}] must be an ordered span`);
      return { start, end };
    }),
    exportAllowed: booleanOf(source['export-allowed'], `${contextPath}.export-allowed`),
  };
}

function decodeStyleRule(value: unknown, contextPath: string): LegacySeriesStyleRule {
  const source = recordFields(
    value,
    ['id', 'instruction', 'exemplar-ids', 'status', 'limitations'],
    [],
    contextPath,
  );
  return {
    id: nonBlankText(source.id, idMax, `${contextPath}.id`),
    instruction: nonBlankText(source.instruction, modelTextMax, `${contextPath}.instruction`),
    exemplarIds: opaqueIdList(source['exemplar-ids'], `${contextPath}.exemplar-ids`),
    status: choiceOf(source.status, ['observed', 'hypothesis'] as const, `${contextPath}.status`),
    limitations: textList(source.limitations, `${contextPath}.limitations`),
  };
}

export function decodeStyleProfile(value: unknown, contextPath: string): LegacySeriesStyleProfile {
  const source = recordFields(
    value,
    [
      'version',
      'reasoning-patterns',
      'voice-rules',
      'format-rules',
      'context-modes',
      'style-exemplar-ids',
      'limitations',
    ],
    [],
    contextPath,
  );
  if (source.version !== 1) invalid(`${contextPath}.version must be 1`);
  const decodeRules = (key: string) => {
    if (!Array.isArray(source[key]) || source[key].length > listMax)
      invalid(`${contextPath}.${key} must be a list of at most ${listMax} rules`);
    return (source[key] as unknown[]).map((item, index) =>
      decodeStyleRule(item, `${contextPath}.${key}[${index}]`),
    );
  };
  if (!Array.isArray(source['context-modes']) || source['context-modes'].length > listMax)
    invalid(`${contextPath}.context-modes must be a list of at most ${listMax} entries`);
  return {
    version: 1,
    reasoningPatterns: decodeRules('reasoning-patterns'),
    voiceRules: decodeRules('voice-rules'),
    formatRules: decodeRules('format-rules'),
    contextModes: (source['context-modes'] as unknown[]).map((item, index) => {
      const mode = recordFields(
        item,
        ['mode', 'status', 'limitations'],
        [],
        `${contextPath}.context-modes[${index}]`,
      );
      return {
        mode: choiceOf(
          mode.mode,
          ['dialogue', 'comment', 'formal'] as const,
          `${contextPath}.context-modes[${index}].mode`,
        ),
        status: choiceOf(
          mode.status,
          ['observed', 'insufficient'] as const,
          `${contextPath}.context-modes[${index}].status`,
        ),
        limitations: textList(
          mode.limitations,
          `${contextPath}.context-modes[${index}].limitations`,
        ),
      };
    }),
    styleExemplarIds: opaqueIdList(
      source['style-exemplar-ids'],
      `${contextPath}.style-exemplar-ids`,
    ),
    limitations: textList(source.limitations, `${contextPath}.limitations`),
  };
}

export function decodeReport(
  value: unknown,
  context: LegacySeriesDecodeContext,
  contextPath: string,
): LegacySeriesReport {
  const optional = context.schemaVersion === 3 ? ['style-exemplars', 'source-coverage'] : [];
  const source = recordFields(
    value,
    ['evidence', 'character', 'thinking-style', 'activity', 'behavior-summary'],
    optional,
    contextPath,
  );
  if (!Array.isArray(source.evidence) || source.evidence.length > listMax)
    invalid(`${contextPath}.evidence must be a list of at most ${listMax} items`);
  const seen = new Set<string>();
  const evidence = source.evidence.map((item: unknown, index: number) => {
    const decoded = decodeEvidence(item, context, `${contextPath}.evidence[${index}]`);
    if (seen.has(decoded.id))
      invalid(`${contextPath}.evidence has a duplicate evidence id '${decoded.id}'`);
    seen.add(decoded.id);
    return decoded;
  });
  const report: LegacySeriesReport = {
    evidence,
    character: nonBlankText(source.character, modelTextMax, `${contextPath}.character`),
    thinkingStyle: nonBlankText(
      source['thinking-style'],
      modelTextMax,
      `${contextPath}.thinking-style`,
    ),
    activity: nonBlankText(source.activity, modelTextMax, `${contextPath}.activity`),
    behaviorSummary: nonBlankText(
      source['behavior-summary'],
      modelTextMax,
      `${contextPath}.behavior-summary`,
    ),
  };
  if (context.schemaVersion === 3 && 'style-exemplars' in source) {
    if (!Array.isArray(source['style-exemplars']) || source['style-exemplars'].length > listMax)
      invalid(`${contextPath}.style-exemplars must be a list of at most ${listMax} items`);
    Object.assign(report, {
      styleExemplars: (source['style-exemplars'] as unknown[]).map((item, index) =>
        decodeStyleExemplar(item, `${contextPath}.style-exemplars[${index}]`),
      ),
    });
  }
  if (context.schemaVersion === 3 && 'source-coverage' in source) {
    if (!Array.isArray(source['source-coverage']) || source['source-coverage'].length > listMax)
      invalid(`${contextPath}.source-coverage must be a list of at most ${listMax} items`);
    Object.assign(report, {
      sourceCoverage: (source['source-coverage'] as unknown[]).map((item, index) =>
        decodeSourceCoverageEntry(item, `${contextPath}.source-coverage[${index}]`),
      ),
    });
  }
  return report;
}

function decodePersonRule(value: unknown, contextPath: string): LegacySeriesPersonRule {
  const source = recordFields(
    value,
    ['id', 'instruction', 'evidence-ids', 'status', 'limitations'],
    [],
    contextPath,
  );
  return {
    id: nonBlankText(source.id, idMax, `${contextPath}.id`),
    instruction: nonBlankText(source.instruction, modelTextMax, `${contextPath}.instruction`),
    evidenceIds: opaqueIdList(source['evidence-ids'], `${contextPath}.evidence-ids`),
    status: choiceOf(
      source.status,
      ['supported', 'hypothesis', 'unknown', 'conflicting'] as const,
      `${contextPath}.status`,
    ),
    limitations: textList(source.limitations, `${contextPath}.limitations`),
  };
}

export function decodePersonProfile(
  value: unknown,
  context: LegacySeriesDecodeContext,
  contextPath: string,
): LegacySeriesPersonProfile {
  const optional =
    context.schemaVersion === 3 ? ['source-context', 'style-profile', 'style-exemplars'] : [];
  const source = recordFields(
    value,
    ['id', 'label', 'palette', 'archetype', 'skills', 'rules', 'task-skills', 'source-status'],
    optional,
    contextPath,
  );
  const decodeRules = (key: string) => {
    if (!Array.isArray(source[key]) || source[key].length > rulesMax)
      invalid(`${contextPath}.${key} must be a list of at most ${rulesMax} rules`);
    return (source[key] as unknown[]).map((item, index) =>
      decodePersonRule(item, `${contextPath}.${key}[${index}]`),
    );
  };
  const profile: LegacySeriesPersonProfile = {
    id: nonBlankText(source.id, idMax, `${contextPath}.id`),
    label: nonBlankText(source.label, labelMax, `${contextPath}.label`),
    palette: choiceOf(source.palette, ['turquoise', 'violet'] as const, `${contextPath}.palette`),
    archetype: nonBlankText(source.archetype, modelTextMax, `${contextPath}.archetype`),
    skills: textList(source.skills, `${contextPath}.skills`),
    rules: decodeRules('rules'),
    taskSkills: decodeRules('task-skills'),
    sourceStatus: choiceOf(
      source['source-status'],
      ['evidence-informed', 'synthetic', 'neutral'] as const,
      `${contextPath}.source-status`,
    ),
  };
  if (context.schemaVersion === 3) {
    if ('source-context' in source) {
      Object.assign(profile, {
        sourceContext: booleanOf(source['source-context'], `${contextPath}.source-context`),
      });
    }
    if ('style-profile' in source) {
      Object.assign(profile, {
        styleProfile: decodeStyleProfile(source['style-profile'], `${contextPath}.style-profile`),
      });
    }
    if ('style-exemplars' in source) {
      if (!Array.isArray(source['style-exemplars']) || source['style-exemplars'].length > listMax)
        invalid(`${contextPath}.style-exemplars must be a list of at most ${listMax} items`);
      Object.assign(profile, {
        styleExemplars: (source['style-exemplars'] as unknown[]).map((item, index) =>
          decodeStyleExemplar(item, `${contextPath}.style-exemplars[${index}]`),
        ),
      });
    }
  }
  return profile;
}

export function decodeArtifacts(
  value: unknown,
  contextPath: string,
): Readonly<Record<string, string>> {
  if (!value || typeof value !== 'object' || Array.isArray(value))
    invalid(`${contextPath} must be an object keyed by artifact names`);
  const source = value as Record<string, unknown>;
  const entries = Object.entries(source);
  if (entries.length > artifactCountMax)
    invalid(`${contextPath} has more than ${artifactCountMax} artifacts`);
  const pairs = entries.map(([name, content]) => {
    if (name.length === 0 || name.length > artifactKeyMax)
      invalid(`${contextPath} has artifact names of 1 to ${artifactKeyMax} characters`);
    return [
      name,
      textOf(content, artifactValueMax, `${contextPath}[${JSON.stringify(name)}]`),
    ] as const;
  });
  // Object.fromEntries defines even inert __proto__/constructor/prototype keys
  // as own data properties on a plain Object.prototype-backed map.
  return Object.fromEntries(pairs);
}

export function decodeExportMetadata(value: unknown): LegacySeriesExportMetadata {
  const source = recordFields(
    value,
    [
      'version',
      'redacted',
      'omitted-style-exemplars',
      'omitted-source-excerpts',
      'preparation-reasoning-omitted',
      'answer-text-may-be-redacted',
      'scores-reference-original-answers',
      'limitations',
    ],
    [],
    'result.export-metadata',
  );
  if (source.version !== 1) invalid("result.export-metadata 'version' must be 1");
  return {
    version: 1,
    redacted: booleanOf(source.redacted, 'result.export-metadata.redacted'),
    omittedStyleExemplars: safeIntegerAtLeast(
      source['omitted-style-exemplars'],
      0,
      'result.export-metadata.omitted-style-exemplars',
    ),
    omittedSourceExcerpts: safeIntegerAtLeast(
      source['omitted-source-excerpts'],
      0,
      'result.export-metadata.omitted-source-excerpts',
    ),
    preparationReasoningOmitted: booleanOf(
      source['preparation-reasoning-omitted'],
      'result.export-metadata.preparation-reasoning-omitted',
    ),
    answerTextMayBeRedacted: booleanOf(
      source['answer-text-may-be-redacted'],
      'result.export-metadata.answer-text-may-be-redacted',
    ),
    scoresReferenceOriginalAnswers: booleanOf(
      source['scores-reference-original-answers'],
      'result.export-metadata.scores-reference-original-answers',
    ),
    limitations: textList(source.limitations, 'result.export-metadata.limitations'),
  };
}
