import type { ExperimentProtocol, SourceGrant, SourcePort } from '@proteus-int/contracts';
import type { StageCallbacks } from '../execution/types';
import type { ExperimentPerson } from './result';

export interface RunOptions extends StageCallbacks {
  readonly sourcePort?: SourcePort;
  readonly sourceGrants?: readonly [readonly SourceGrant[], readonly SourceGrant[]];
  readonly repetitions?: 1 | 3;
  readonly onProtocol?: (protocol: ExperimentProtocol) => Promise<void>;
  readonly onPerson?: (person: ExperimentPerson) => void;
}
