export interface ScoreInterval {
  readonly lower: number;
  readonly upper: number;
}
export interface RubricCriterion {
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
export interface EvaluationRubric {
  readonly version: 2;
  readonly criteria: readonly RubricCriterion[];
}
export type CriterionStatus = 'unmet' | 'partial' | 'met' | 'unknown';
export interface CriterionAssessment {
  readonly 'criterion-id': string;
  readonly status: CriterionStatus;
  readonly reason: string;
  readonly quote: string;
}
export interface AnswerEvaluation {
  readonly 'person-id': string;
  readonly criteria: readonly CriterionAssessment[];
}
export type RequiredStatus = 'passed' | 'failed' | 'unknown';
export interface CriterionScore {
  readonly 'criterion-id': string;
  readonly interval: ScoreInterval;
  readonly assessments: readonly CriterionAssessment[];
  readonly disputed: boolean;
}
export interface TrialScore {
  readonly version: 2;
  readonly 'person-id': string;
  readonly interval: ScoreInterval;
  readonly criteria: readonly CriterionScore[];
  readonly 'required-status': RequiredStatus;
  readonly 'judge-disagreement': boolean;
}
export interface ScoreSummary {
  readonly version: 2;
  readonly 'person-id': string;
  readonly interval: ScoreInterval | null;
  readonly point: number | null;
  readonly planned: number;
  readonly evaluated: number;
  readonly complete: boolean;
  readonly spread: ScoreInterval | null;
  readonly 'required-status': RequiredStatus;
  readonly 'judge-disagreement': boolean;
  readonly 'comparison-eligible': boolean;
}
