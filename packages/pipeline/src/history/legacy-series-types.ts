// Own camelCase view of the exact legacy series-result schemas:
//  - schema-version 2, prompt bsc-v2.1 (raw save, no style/source fields);
//  - schema-version 3, prompts bsc-v2.2-source-style / bsc-v2.3-source-identity
//    (raw save and redacted export, optional style/source fields, reply v1,
//    export-metadata v1 on the export variant).
// Both eras use scoring-version 2, protocol version 2, rubric version 2 and
// reply/style version 1. Recorded BSC values are preserved verbatim.

export type LegacySeriesSchemaVersion = 2 | 3;
export type LegacySeriesScoringVersion = 2;
export type LegacySeriesProtocolVersion = 2;
export type LegacySeriesRubricVersion = 2;
export type LegacySeriesStyleVersion = 1;
export type LegacySeriesReplyVersion = 1;
export type LegacySeriesExportMetadataVersion = 1;

export type LegacySeriesStatus = 'complete' | 'incomplete' | 'cancelled';
export type LegacySeriesPalette = 'turquoise' | 'violet';
export type LegacySeriesProvider = 'codex' | 'kimi';
export type LegacySeriesEffort = 'low' | 'medium' | 'high';
export type LegacySeriesComponentId = 'stalker' | 'person-fabric' | 'person' | 'brain-analyzator';
export type LegacySeriesPromptVersion =
  'bsc-v2.1' | 'bsc-v2.2-source-style' | 'bsc-v2.3-source-identity';
export type LegacySeriesEvidenceStatus = 'supported' | 'hypothesis' | 'unknown' | 'conflicting';
export type LegacySeriesVerification = 'model-reported' | 'source-retrieved' | 'unverified';
export type LegacySeriesCriterionStatus = 'unmet' | 'partial' | 'met' | 'unknown';
export type LegacySeriesRequiredStatus = 'passed' | 'failed' | 'unknown';
export type LegacySeriesTrialStatus = 'pending' | 'answered' | 'evaluated' | 'failed' | 'skipped';
export type LegacySeriesAttemptStatus = 'returned' | 'format-rejected' | 'accepted' | 'failed';
export type LegacySeriesCommunicationMode = 'dialogue' | 'comment' | 'formal';
export type LegacySeriesSourceStatus = 'evidence-informed' | 'synthetic' | 'neutral';
export type LegacySeriesReasoningKind = 'summary' | 'thinking';
export type LegacySeriesStyleRuleStatus = 'observed' | 'hypothesis';
export type LegacySeriesContextModeStatus = 'observed' | 'insufficient';
export type LegacySeriesParticipantId = 'a' | 'b' | 'neutral';

export interface LegacySeriesPersonInput {
  readonly label: string;
  readonly context: string;
  readonly publicUrls: readonly string[];
}

export interface LegacySeriesExperimentInput {
  readonly task: string;
  readonly people: readonly [LegacySeriesPersonInput, LegacySeriesPersonInput];
}

export interface LegacySeriesScoreInterval {
  readonly lower: number;
  readonly upper: number;
}

export interface LegacySeriesRubricCriterion {
  readonly id: string;
  readonly label: string;
  readonly weight: number;
  readonly required: boolean;
  readonly anchors: {
    readonly unmet: string;
    readonly partial: string;
    readonly met: string;
  };
}

export interface LegacySeriesRubric {
  readonly version: LegacySeriesRubricVersion;
  readonly criteria: readonly LegacySeriesRubricCriterion[];
}

export interface LegacySeriesModelSetting {
  readonly provider: LegacySeriesProvider;
  readonly requestedModel: string;
  readonly requestedEffort: LegacySeriesEffort;
}

export interface LegacySeriesProtocol {
  readonly version: LegacySeriesProtocolVersion;
  readonly task: string;
  readonly rubric: LegacySeriesRubric;
  readonly repetitions: 1 | 3;
  readonly participantOrder: readonly (readonly string[])[];
  readonly modelSettings: Readonly<Record<LegacySeriesComponentId, LegacySeriesModelSetting>>;
  readonly formatAttempts: 2;
  readonly stageTimeoutMs: 600000;
  readonly promptVersion: LegacySeriesPromptVersion;
  readonly createdAt: string;
  readonly sourceSnapshots?: readonly (readonly string[])[];
}

export interface LegacySeriesReasoningBlock {
  readonly id: string;
  readonly kind: LegacySeriesReasoningKind;
  readonly text: string;
}

export interface LegacySeriesIntelligenceTrace {
  readonly provider: LegacySeriesProvider;
  readonly reasoningMs: number;
  readonly wallTimeMs: number;
  readonly terminalMs: number;
  readonly summary: string;
  readonly reasoning: readonly LegacySeriesReasoningBlock[];
  readonly requestedModel: string;
  readonly requestedEffort: LegacySeriesEffort;
  readonly resolvedModel: string | null;
  readonly appliedEffort: LegacySeriesEffort | null;
  readonly cliVersion: string | null;
  readonly activeReasoningMs: null;
  readonly reasoningTruncated: boolean;
}

export interface LegacySeriesInvocationAttempt {
  readonly index: number;
  readonly status: LegacySeriesAttemptStatus;
  readonly wallTimeMs: number;
  readonly error: string | null;
}

export interface LegacySeriesPersonReply {
  readonly version: LegacySeriesReplyVersion;
  readonly messages: readonly string[];
  readonly styleConflicts: readonly string[];
}

export interface LegacySeriesAttempt {
  readonly answer: string;
  readonly trace: LegacySeriesIntelligenceTrace;
  readonly reply?: LegacySeriesPersonReply;
}

export interface LegacySeriesEvidence {
  readonly id: string;
  readonly sourceUrl: string;
  readonly kind: 'fact' | 'hypothesis';
  readonly summary: string;
  readonly status: LegacySeriesEvidenceStatus;
  readonly verification: LegacySeriesVerification;
  readonly excerpt: string | null;
  readonly retrievedAt: string | null;
  readonly documentIds?: readonly string[];
  readonly limitations: readonly string[];
}

export interface LegacySeriesSourceCoverage {
  readonly documents: number;
  readonly totalDocuments: number | null;
  readonly bytes: number;
  readonly complete: boolean;
  readonly truncated: boolean;
  readonly notAnalyzed: number;
  readonly unavailable: readonly string[];
}

export interface LegacySeriesSourceCoverageEntry {
  readonly snapshotId: string;
  readonly coverage: LegacySeriesSourceCoverage;
  readonly analyzedDocuments: number;
  readonly limitations: readonly string[];
}

export interface LegacySeriesReport {
  readonly evidence: readonly LegacySeriesEvidence[];
  readonly character: string;
  readonly thinkingStyle: string;
  readonly activity: string;
  readonly behaviorSummary: string;
  readonly styleExemplars?: readonly LegacySeriesStyleExemplar[];
  readonly sourceCoverage?: readonly LegacySeriesSourceCoverageEntry[];
}

export interface LegacySeriesPersonRule {
  readonly id: string;
  readonly instruction: string;
  readonly evidenceIds: readonly string[];
  readonly status: LegacySeriesEvidenceStatus;
  readonly limitations: readonly string[];
}

export interface LegacySeriesStyleRule {
  readonly id: string;
  readonly instruction: string;
  readonly exemplarIds: readonly string[];
  readonly status: LegacySeriesStyleRuleStatus;
  readonly limitations: readonly string[];
}

export interface LegacySeriesContextMode {
  readonly mode: LegacySeriesCommunicationMode;
  readonly status: LegacySeriesContextModeStatus;
  readonly limitations: readonly string[];
}

export interface LegacySeriesStyleProfile {
  readonly version: LegacySeriesStyleVersion;
  readonly reasoningPatterns: readonly LegacySeriesStyleRule[];
  readonly voiceRules: readonly LegacySeriesStyleRule[];
  readonly formatRules: readonly LegacySeriesStyleRule[];
  readonly contextModes: readonly LegacySeriesContextMode[];
  readonly styleExemplarIds: readonly string[];
  readonly limitations: readonly string[];
}

export interface LegacySeriesRedactionSpan {
  readonly start: number;
  readonly end: number;
}

export interface LegacySeriesStyleExemplar {
  readonly id: string;
  readonly documentId: string;
  readonly mode: LegacySeriesCommunicationMode;
  readonly text: string;
  readonly context: string;
  readonly redactions: readonly LegacySeriesRedactionSpan[];
  readonly exportAllowed: boolean;
}

export interface LegacySeriesPersonProfile {
  readonly id: string;
  readonly label: string;
  readonly palette: LegacySeriesPalette;
  readonly archetype: string;
  readonly skills: readonly string[];
  readonly rules: readonly LegacySeriesPersonRule[];
  readonly taskSkills: readonly LegacySeriesPersonRule[];
  readonly sourceStatus: LegacySeriesSourceStatus;
  readonly sourceContext?: boolean;
  readonly styleProfile?: LegacySeriesStyleProfile;
  readonly styleExemplars?: readonly LegacySeriesStyleExemplar[];
}

export interface LegacySeriesCriterionAssessment {
  readonly criterionId: string;
  readonly status: LegacySeriesCriterionStatus;
  readonly reason: string;
  readonly quote: string;
}

export interface LegacySeriesAnswerEvaluation {
  readonly personId: string;
  readonly criteria: readonly LegacySeriesCriterionAssessment[];
}

export interface LegacySeriesCriterionScore {
  readonly criterionId: string;
  readonly interval: LegacySeriesScoreInterval;
  readonly assessments: readonly LegacySeriesCriterionAssessment[];
  readonly disputed: boolean;
}

export interface LegacySeriesTrialScore {
  readonly version: LegacySeriesScoringVersion;
  readonly personId: string;
  readonly interval: LegacySeriesScoreInterval;
  readonly criteria: readonly LegacySeriesCriterionScore[];
  readonly requiredStatus: LegacySeriesRequiredStatus;
  readonly judgeDisagreement: boolean;
}

export interface LegacySeriesScoreSummary {
  readonly version: LegacySeriesScoringVersion;
  readonly personId: string;
  readonly interval: LegacySeriesScoreInterval | null;
  readonly point: number | null;
  readonly planned: number;
  readonly evaluated: number;
  readonly complete: boolean;
  readonly spread: LegacySeriesScoreInterval | null;
  readonly requiredStatus: LegacySeriesRequiredStatus;
  readonly judgeDisagreement: boolean;
  readonly comparisonEligible: boolean;
}

export interface LegacySeriesTrial {
  readonly id: string;
  readonly round: number;
  readonly personId: string;
  readonly status: LegacySeriesTrialStatus;
  readonly attempt: LegacySeriesAttempt | null;
  readonly score: LegacySeriesTrialScore | null;
  readonly evaluations: readonly LegacySeriesAnswerEvaluation[];
  readonly error: string | null;
}

export interface LegacySeriesExperimentPerson {
  readonly report: LegacySeriesReport | null;
  readonly baseProfile: LegacySeriesPersonProfile;
  readonly profile: LegacySeriesPersonProfile;
  readonly attempt: LegacySeriesAttempt | null;
  readonly score: LegacySeriesScoreSummary;
  readonly trials: readonly LegacySeriesTrial[];
  readonly preparationWallTimeMs: number;
  readonly solutionWallTimeMs: number;
  readonly baselineDifference: LegacySeriesScoreInterval | null;
  readonly artifacts: Readonly<Record<string, string>>;
}

export interface LegacySeriesStageTrace {
  readonly id: string;
  readonly stage: LegacySeriesComponentId;
  readonly person: number | null;
  readonly label: string;
  readonly trace: LegacySeriesIntelligenceTrace;
  readonly attempts: readonly LegacySeriesInvocationAttempt[];
}

export interface LegacySeriesComparison {
  readonly summary: string;
  readonly difference: LegacySeriesScoreInterval | null;
  readonly comparisonEligible: boolean;
}

export interface LegacySeriesExportMetadata {
  readonly version: LegacySeriesExportMetadataVersion;
  readonly redacted: boolean;
  readonly omittedStyleExemplars: number;
  readonly omittedSourceExcerpts: number;
  readonly preparationReasoningOmitted: boolean;
  readonly answerTextMayBeRedacted: boolean;
  readonly scoresReferenceOriginalAnswers: boolean;
  readonly limitations: readonly string[];
}

export interface LegacySeriesResult {
  readonly schemaVersion: LegacySeriesSchemaVersion;
  readonly scoringVersion: LegacySeriesScoringVersion;
  readonly status: LegacySeriesStatus;
  readonly input: LegacySeriesExperimentInput;
  readonly protocol: LegacySeriesProtocol;
  readonly people: readonly [LegacySeriesExperimentPerson, LegacySeriesExperimentPerson];
  readonly control: LegacySeriesExperimentPerson;
  readonly trials: readonly LegacySeriesTrial[];
  readonly comparison: LegacySeriesComparison;
  readonly traces: readonly LegacySeriesStageTrace[];
  readonly finishedAt: string;
  readonly exportMetadata?: LegacySeriesExportMetadata;
}

/** Era-aware decode inputs shared by the per-object decoders. */
export interface LegacySeriesDecodeContext {
  readonly schemaVersion: LegacySeriesSchemaVersion;
  readonly repetitions: 1 | 3;
  readonly rubricCriteria: readonly LegacySeriesRubricCriterion[];
}
