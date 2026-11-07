import type { Evidence, StyleExemplar, SourceCoverage } from '@proteus-int/contracts';

export interface StalkerReport {
  readonly evidence: readonly Evidence[];
  readonly character: string;
  readonly thinkingStyle: string;
  readonly activity: string;
  readonly behaviorSummary: string;
  readonly styleExemplars?: readonly StyleExemplar[];
  readonly sourceCoverage?: readonly {
    readonly snapshotId: string;
    readonly coverage: SourceCoverage;
    readonly analyzedDocuments: number;
    readonly limitations: readonly string[];
  }[];
}

export function mergeSourceReports(
  reports: readonly StalkerReport[],
  exemplars: readonly StyleExemplar[],
  coverage: NonNullable<StalkerReport['sourceCoverage']>[number][],
): StalkerReport {
  const join = (field: 'character' | 'thinkingStyle' | 'activity' | 'behaviorSummary') => {
    const value = [
      ...new Set(reports.map((report) => report[field]).filter((item) => item !== 'unknown')),
    ].join('\n');
    if (value.length > 24000)
      coverage.forEach((item) => {
        (item.limitations as string[]).push('Behavior summary truncated at representation limit');
      });
    return value.slice(0, 24000) || 'unknown';
  };
  return {
    evidence: reports.flatMap((report) => report.evidence),
    character: join('character'),
    thinkingStyle: join('thinkingStyle'),
    activity: join('activity'),
    behaviorSummary: join('behaviorSummary'),
    styleExemplars: exemplars,
    sourceCoverage: coverage,
  };
}
