import type { Evidence, StyleExemplar } from '@proteus-int/contracts';

/** Export-side metadata appended by exportExperiment; absent on raw saved results. */
export interface StoredExportMetadata {
  readonly version: 1;
  readonly redacted: boolean;
  readonly omittedStyleExemplars: number;
  readonly omittedSourceExcerpts: number;
  readonly preparationReasoningOmitted: boolean;
  readonly answerTextMayBeRedacted: boolean;
  readonly scoresReferenceOriginalAnswers: true;
  readonly limitations: readonly string[];
}

interface SourceProfile {
  readonly sourceContext?: boolean;
  readonly styleExemplars?: readonly StyleExemplar[];
}
interface SourceReport {
  readonly evidence: readonly Evidence[];
  readonly styleExemplars?: readonly StyleExemplar[];
  readonly sourceCoverage?: readonly unknown[];
}
interface SourceParticipant {
  readonly profile: SourceProfile;
  readonly report: SourceReport | null;
}
interface SourceTrace {
  readonly stage: string;
  readonly trace: { readonly reasoning?: readonly unknown[]; readonly summary: string };
}

/** Shared export policy, applied only to a separate view of an already decoded snapshot. */
export function createSourceExportPolicy(participants: readonly SourceParticipant[]) {
  const privateSamples: StyleExemplar[] = [];
  const privateExcerpts: string[] = [];
  const connected = participants.some(
    (person) =>
      person.profile.sourceContext ||
      person.report?.sourceCoverage?.length ||
      person.report?.evidence.some((item) => item.verification === 'source-retrieved'),
  );
  let textRedacted = false;
  const samples = (values: readonly StyleExemplar[]) =>
    values.filter((sample) => {
      if (sample.exportAllowed) return true;
      privateSamples.push(sample);
      return false;
    });
  function profile<T extends SourceProfile>(value: T): T {
    return {
      ...value,
      ...(value.styleExemplars ? { styleExemplars: samples(value.styleExemplars) } : {}),
    };
  }
  function report<T extends SourceReport>(value: T | null): T | null {
    if (!value) return null;
    return {
      ...value,
      evidence: value.evidence.map((item) => {
        if (item.verification !== 'source-retrieved' || item.excerpt === null) return item;
        privateExcerpts.push(item.excerpt);
        return {
          ...item,
          excerpt: null,
          limitations: [...item.limitations, 'Source excerpt omitted from export'],
        };
      }),
      ...(value.styleExemplars ? { styleExemplars: samples(value.styleExemplars) } : {}),
    };
  }
  function traces<T extends SourceTrace>(values: readonly T[]): readonly T[] {
    return values.map((entry) =>
      (connected || privateSamples.length) &&
      (entry.stage === 'stalker' || entry.stage === 'person-fabric')
        ? {
            ...entry,
            trace: {
              ...entry.trace,
              reasoning: [],
              summary: '[private preparation trace omitted from export]',
            },
          }
        : entry,
    );
  }
  function serialize(value: unknown): string {
    const snippets = [
      ...new Set(
        [
          ...privateSamples.flatMap((sample) => [sample.text, sample.context]),
          ...privateExcerpts,
        ].filter((text) => text.length >= 20),
      ),
    ].sort((a, b) => b.length - a.length);
    return JSON.stringify(value, (_key, value: unknown) => {
      if (typeof value !== 'string') return value;
      let clean = value;
      for (const snippet of snippets) {
        if (clean.includes(snippet)) {
          clean = clean.replaceAll(snippet, '[private source text omitted from export]');
          textRedacted = true;
        }
      }
      return clean;
    });
  }
  function metadata(): StoredExportMetadata {
    return {
      version: 1,
      redacted: connected || privateSamples.length > 0,
      omittedStyleExemplars: [
        ...new Set(privateSamples.map((sample) => `${sample.documentId}:${sample.id}`)),
      ].length,
      omittedSourceExcerpts: privateExcerpts.length,
      preparationReasoningOmitted: connected || privateSamples.length > 0,
      answerTextMayBeRedacted: textRedacted,
      scoresReferenceOriginalAnswers: true,
      limitations:
        connected || privateSamples.length
          ? [
              'Connected-source excerpts and private sample objects with their context are omitted. Matching text of at least 20 characters is replaced elsewhere; short quotations inside generated answers and paraphrases are not detected.',
              'Scores describe the original in-memory answers; redacted exported answers are not a lossless replay.',
            ]
          : [],
    };
  }
  return { profile, report, traces, serialize, metadata };
}
