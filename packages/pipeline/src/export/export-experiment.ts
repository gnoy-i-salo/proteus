import { decodeExecutionSnapshot } from '@proteus-int/contracts';
import { personaArtifacts } from '@proteus-int/person';
import type { ExperimentPerson, ExperimentResult } from '../experiment/result';
import { createSourceExportPolicy } from './source-export-policy';

export function exportExperiment(result: ExperimentResult): string {
  decodeExecutionSnapshot(result.execution);
  const policy = createSourceExportPolicy([...result.people, result.control]);
  const participant = (value: ExperimentPerson): ExperimentPerson => {
    const profile = policy.profile(value.profile);
    return {
      ...value,
      profile,
      baseProfile: policy.profile(value.baseProfile),
      report: policy.report(value.report),
      artifacts: personaArtifacts(profile),
    };
  };
  const people = result.people.map(participant);
  const control = participant(result.control);
  const body = policy.serialize({
    ...result,
    people,
    control,
    traces: policy.traces(result.traces),
  });
  const exported = JSON.parse(body) as Record<string, unknown>;
  return JSON.stringify({ ...exported, exportMetadata: policy.metadata() }, null, 2);
}
