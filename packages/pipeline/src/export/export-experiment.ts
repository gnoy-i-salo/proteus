import { decodeExecutionSnapshot } from '@proteus-int/contracts';
import type { PersonProfile, StyleExemplar } from '@proteus-int/contracts';
import { personaArtifacts } from '@proteus-int/person';
import type { ExperimentPerson, ExperimentResult } from '../experiment/result';

export function exportExperiment(result: ExperimentResult): string {
  decodeExecutionSnapshot(result.execution);
  const privateSamples: StyleExemplar[] = [];
  const privateExcerpts: string[] = [];
  const connected = [...result.people, result.control].some(
    (person) =>
      person.profile.sourceContext ||
      person.report?.sourceCoverage?.length ||
      person.report?.evidence.some((item) => item.verification === 'source-retrieved'),
  );
  const samples = (values: readonly StyleExemplar[] | undefined) =>
    values?.filter((sample) => {
      if (sample.exportAllowed) return true;
      privateSamples.push(sample);
      return false;
    });
  const profile = (value: PersonProfile): PersonProfile => ({
    ...value,
    ...(value.styleExemplars ? { styleExemplars: samples(value.styleExemplars)! } : {}),
  });
  const participant = (value: ExperimentPerson): ExperimentPerson => {
    const cleanProfile = profile(value.profile);
    return {
      ...value,
      profile: cleanProfile,
      baseProfile: profile(value.baseProfile),
      report: value.report
        ? {
            ...value.report,
            evidence: value.report.evidence.map((item) => {
              if (item.verification !== 'source-retrieved' || item.excerpt === null) return item;
              privateExcerpts.push(item.excerpt);
              return {
                ...item,
                excerpt: null,
                limitations: [...item.limitations, 'Source excerpt omitted from export'],
              };
            }),
            ...(value.report.styleExemplars
              ? { styleExemplars: samples(value.report.styleExemplars)! }
              : {}),
          }
        : null,
      artifacts: personaArtifacts(cleanProfile),
    };
  };
  const people = result.people.map(participant);
  const control = participant(result.control);
  const snippets = [
    ...new Set(
      [
        ...privateSamples.flatMap((sample) => [sample.text, sample.context]),
        ...privateExcerpts,
      ].filter((value) => value.length >= 20),
    ),
  ].sort((a, b) => b.length - a.length);
  let textRedacted = false;
  const traces = result.traces.map((entry) =>
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
  const body = JSON.stringify({ ...result, people, control, traces }, (_key, value: unknown) => {
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
  const exported = JSON.parse(body) as Record<string, unknown>;
  return JSON.stringify(
    {
      ...exported,
      exportMetadata: {
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
      },
    },
    null,
    2,
  );
}
