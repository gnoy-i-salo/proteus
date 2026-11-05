import type { IntelligenceTrace, PersonScore } from '@proteus-int/contracts';
import type { ScoringInput } from '@proteus-int/brain-scoring';
import type { IntelligenceProvider } from '@proteus-int/intelligence';
export interface BrainAnalysis {
  readonly summary: string;
  readonly scores: readonly PersonScore[];
  readonly 'key-differences': readonly string[];
}
export interface BrainAnalyzatorPort {
  analyze(
    scores: readonly PersonScore[],
    traces: readonly IntelligenceTrace[],
    provider: IntelligenceProvider,
    signal?: AbortSignal,
  ): Promise<BrainAnalysis>;
}
export type AnalysisScoringInput = readonly ScoringInput[];
