import type { StageLifecycle } from '@proteus-int/contracts';
import type { StageTrace } from '@proteus-int/pipeline';
export type ExecutionStageIdentity = Pick<
  StageLifecycle,
  'logicalStageId' | 'component' | 'person' | 'label'
>;
export interface ExecutionStageClassification {
  readonly participant: 'a' | 'b' | 'control' | 'system' | 'unknown';
  readonly round: number | null;
  readonly phase:
    'protocol' | 'research' | 'profile' | 'adaptation' | 'solution' | 'evaluation' | 'unknown';
  readonly componentLabel: string;
}
function participantFor(person: number | null): ExecutionStageClassification['participant'] {
  switch (person) {
    case 0:
      return 'a';
    case 1:
      return 'b';
    case 2:
      return 'control';
    case null:
      return 'system';
    default:
      return 'unknown';
  }
}
function componentName(component: string): string {
  switch (component) {
    case 'stalker':
      return 'STALKER';
    case 'person-fabric':
      return 'Person Fabric';
    case 'person':
      return 'Person';
    case 'brain-analyzator':
      return 'Brain Analyzator';
    default:
      return component || 'Неизвестный компонент';
  }
}
function phaseFor(
  stage: ExecutionStageIdentity,
): Pick<ExecutionStageClassification, 'phase' | 'round'> {
  const id = stage.logicalStageId;
  if (stage.component === 'brain-analyzator' && stage.person === null) {
    if (id === 'rubric') return { phase: 'protocol', round: null };
    const evaluation = /^judge-([1-3])-[01]$/.exec(id);
    if (evaluation) return { phase: 'evaluation', round: Number(evaluation[1]) };
  }
  if (stage.component === 'stalker') {
    const research = /^sources-([01])$/.exec(id);
    if (research && Number(research[1]) === stage.person) return { phase: 'research', round: null };
  }
  if (stage.component === 'person-fabric') {
    const profile = /^profile-([01])$/.exec(id);
    if (profile && Number(profile[1]) === stage.person) return { phase: 'profile', round: null };
    const adaptation = /^adapt-([012])$/.exec(id);
    if (adaptation && Number(adaptation[1]) === stage.person)
      return { phase: 'adaptation', round: null };
  }
  if (stage.component === 'person') {
    const solution = /^solve-([1-3])-(a|b|neutral)$/.exec(id);
    if (solution) {
      const person = solution[2] === 'a' ? 0 : solution[2] === 'b' ? 1 : 2;
      if (person === stage.person) return { phase: 'solution', round: Number(solution[1]) };
    }
  }
  return { phase: 'unknown', round: null };
}
export function classifyExecutionStage(
  stage: ExecutionStageIdentity,
): ExecutionStageClassification {
  return {
    participant: participantFor(stage.person),
    ...phaseFor(stage),
    componentLabel: componentName(stage.component),
  };
}
export function classifyTraceStage(
  trace: Pick<StageTrace, 'id' | 'stage' | 'person' | 'label' | 'lifecycle'>,
): ExecutionStageClassification {
  return classifyExecutionStage(
    trace.lifecycle ?? {
      logicalStageId: trace.id,
      component: trace.stage,
      person: trace.person,
      label: trace.label,
    },
  );
}
