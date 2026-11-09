import type {
  AnswerEvaluation,
  ExperimentInput,
  ExecutionSnapshot,
  ExperimentProtocol,
  IntelligenceResult,
  PersonProfile,
  PersonaTaskAdaptation,
  ScoreInterval,
  ScoreSummary,
  TrialScore,
} from '@proteus-int/contracts';
import type { StalkerReport } from '@proteus-int/stalker';
import { personaArtifacts } from '@proteus-int/person';
import { difference, summarizeScores } from '@proteus-int/brain-scoring';
import { duration } from '../execution/collect-traces';
import type { StageTrace } from '../execution/types';
import type { PreparedPerson } from './prepare-people';

export interface TrialResult {
  readonly id: string;
  readonly round: number;
  readonly personId: string;
  readonly status: 'pending' | 'answered' | 'evaluated' | 'failed' | 'skipped';
  readonly attempt: IntelligenceResult | null;
  readonly score: TrialScore | null;
  readonly evaluations: readonly AnswerEvaluation[];
  readonly error: string | null;
}
export interface ExperimentPerson {
  /** Required by schemaVersion 5; legacy v4 uses profile.id for the series role. */
  readonly participantId?: 'a' | 'b' | 'neutral';
  readonly taskAdaptation?: PersonaTaskAdaptation;
  readonly report: StalkerReport | null;
  readonly baseProfile: PersonProfile;
  readonly profile: PersonProfile;
  readonly attempt: IntelligenceResult | null;
  readonly score: ScoreSummary;
  readonly trials: readonly TrialResult[];
  readonly preparationWallTimeMs: number;
  readonly solutionWallTimeMs: number;
  readonly baselineDifference: ScoreInterval | null;
  readonly artifacts: Readonly<Record<string, string>>;
}
export interface ExperimentResult {
  readonly schemaVersion: 4 | 5;
  /** Absent for historical schema4 results without lifecycle observations. */
  readonly execution?: ExecutionSnapshot;
  readonly scoringVersion: 2;
  readonly status: 'complete' | 'incomplete' | 'cancelled';
  readonly input: ExperimentInput;
  readonly protocol: ExperimentProtocol;
  readonly people: readonly ExperimentPerson[];
  readonly control: ExperimentPerson;
  readonly trials: readonly TrialResult[];
  readonly comparison: {
    readonly summary: string;
    readonly difference: ScoreInterval | null;
    readonly comparisonEligible: boolean;
  };
  readonly finishedAt: string;
  readonly traces: readonly StageTrace[];
}

export function buildExperimentResult(
  input: ExperimentInput,
  protocol: ExperimentProtocol,
  prepared: readonly PreparedPerson[],
  trials: readonly TrialResult[],
  status: ExperimentResult['status'],
  traces: readonly StageTrace[],
): ExperimentResult {
  const repetitions = protocol.repetitions;
  const participants = prepared.map((person, index): ExperimentPerson => {
    const ownTrials = trials.filter((trial) => trial.personId === person.participantId);
    return {
      participantId: person.participantId,
      ...(person.taskAdaptation ? { taskAdaptation: person.taskAdaptation } : {}),
      report: person.report,
      baseProfile: person.base,
      profile: person.profile,
      attempt: ownTrials.find((trial) => trial.attempt)?.attempt ?? null,
      score: summarizeScores(
        person.participantId,
        ownTrials.flatMap((trial) => (trial.score ? [trial.score] : [])),
        repetitions,
      ),
      trials: ownTrials,
      artifacts: personaArtifacts(person.profile),
      baselineDifference: null,
      preparationWallTimeMs: traces
        .filter((item) => item.person === index && item.stage !== 'person')
        .reduce((sum, item) => sum + duration(item.trace), 0),
      solutionWallTimeMs: traces
        .filter((item) => item.person === index && item.stage === 'person')
        .reduce((sum, item) => sum + duration(item.trace), 0),
    };
  });
  const control = participants[2]!;
  const people = participants.slice(0, 2).map((person) => ({
    ...person,
    baselineDifference:
      person.score.interval && control.score.interval
        ? difference(person.score.interval, control.score.interval)
        : null,
  }));
  const a = people[0]!.score;
  const b = people[1]!.score;
  const eligible =
    status === 'complete' &&
    repetitions === 3 &&
    participants.every((person) => person.score.comparisonEligible);
  const summary =
    status !== 'complete'
      ? 'Серия не завершена. Баллы относятся только к оценённым ответам; итоговое сравнение недоступно.'
      : repetitions === 1
        ? 'Предварительный эксперимент: один прогон. Баллы описывают выполнение критериев, устойчивость не установлена.'
        : eligible
          ? 'Описательное сравнение трёх прогонов в одинаковых условиях. Это не доказательство статистического превосходства.'
          : 'Сравнение ограничено: есть непроверенные критерии, расхождение оценок или невыполненные обязательные требования.';
  const result: ExperimentResult = {
    schemaVersion: 5,
    scoringVersion: 2,
    status,
    input,
    protocol,
    people,
    control,
    trials,
    comparison: {
      summary,
      comparisonEligible: eligible,
      difference: a.interval && b.interval ? difference(a.interval, b.interval) : null,
    },
    traces,
    finishedAt: new Date().toISOString(),
  };
  return result;
}
