import type {
  CorpusSnapshot,
  OpenResearchSessionRequest,
  PersonInput,
} from '@proteus-int/contracts';
import { decodeCorpusSnapshot, decodeOpenResearchSessionRequest } from '@proteus-int/contracts';

export interface ResearchSample {
  readonly episodeId: string;
  readonly sourceKey: string;
  readonly periodKey: string;
}
export interface MemoryResearchInput {
  readonly person: PersonInput;
  readonly request: OpenResearchSessionRequest;
  readonly snapshot: CorpusSnapshot;
  // Host-selected reference candidates. These hints never grant native access.
  readonly samples: readonly ResearchSample[];
  readonly limits: {
    readonly maxSeedEpisodes: number;
    readonly maxPlanningSteps: number;
    readonly maxItemsPerBatch: number;
  };
}
function text(value: string, maximum: number, nonblank = false): string {
  const bytes = new TextEncoder().encode(value);
  if (
    typeof value !== 'string' ||
    bytes.length > maximum ||
    new TextDecoder('utf-8', { fatal: true, ignoreBOM: true }).decode(bytes) !== value ||
    (nonblank && (!value.trim() || /\p{Cc}/u.test(value)))
  )
    throw new Error('Неверный текст параметров исследования.');
  return value;
}
function count(value: number, minimum: number, maximum: number): number {
  if (!Number.isSafeInteger(value) || value < minimum || value > maximum)
    throw new Error('Неверный предел исследования.');
  return value;
}
export function validateResearchInput(input: MemoryResearchInput): MemoryResearchInput {
  const request = decodeOpenResearchSessionRequest(input.request);
  const snapshot = decodeCorpusSnapshot(input.snapshot);
  if (
    snapshot.id !== request.snapshotId ||
    input.samples.length > 10000 ||
    input.person.publicUrls.length > 32 ||
    request.modelBudget.maxCalls < 2
  )
    throw new Error('Исследование не соответствует снимку или бюджету.');
  const episodes = new Set(snapshot.episodeIds);
  const selected = new Set<string>();
  const samples = input.samples.map((sample) => {
    if (!episodes.has(sample.episodeId) || selected.has(sample.episodeId))
      throw new Error('Эпизод выборки отсутствует в снимке или повторяется.');
    selected.add(sample.episodeId);
    return {
      episodeId: text(sample.episodeId, 256, true),
      sourceKey: text(sample.sourceKey, 256, true),
      periodKey: text(sample.periodKey, 64, true),
    };
  });
  const person = {
    label: text(input.person.label, 512, true),
    context: text(input.person.context, 20000),
    publicUrls: input.person.publicUrls.map((url) => text(url, 2048, true)),
  };
  if (new TextEncoder().encode(JSON.stringify(person)).length > 30000)
    throw new Error('Описание персоны превышает размер исследовательского контекста.');
  return {
    request,
    snapshot,
    samples,
    person,
    limits: {
      maxSeedEpisodes: count(
        input.limits.maxSeedEpisodes,
        1,
        Math.min(100, request.recallBudget.maxCalls),
      ),
      maxPlanningSteps: count(input.limits.maxPlanningSteps, 0, 20),
      maxItemsPerBatch: count(input.limits.maxItemsPerBatch, 1, 100),
    },
  };
}
function interleave<T>(groups: readonly (readonly T[])[], limit: number): T[] {
  const result: T[] = [];
  for (let position = 0; result.length < limit; position++) {
    const before = result.length;
    for (const group of groups) {
      if (result.length === limit) break;
      const value = group[position];
      if (value !== undefined) result.push(value);
    }
    if (result.length === before) break;
  }
  return result;
}
export function selectResearchSamples(input: MemoryResearchInput): readonly ResearchSample[] {
  const sources = new Map<string, Map<string, ResearchSample[]>>();
  for (const sample of input.samples) {
    let periods = sources.get(sample.sourceKey);
    if (!periods) {
      periods = new Map();
      sources.set(sample.sourceKey, periods);
    }
    const values = periods.get(sample.periodKey);
    if (values) values.push(sample);
    else periods.set(sample.periodKey, [sample]);
  }
  const limit = input.limits.maxSeedEpisodes;
  const groups = [...sources.keys()].sort().map((source) => {
    const periods = sources.get(source)!;
    const groups = [...periods.keys()]
      .sort()
      .map((period) =>
        periods
          .get(period)!
          .sort((a, b) => (a.episodeId < b.episodeId ? -1 : a.episodeId > b.episodeId ? 1 : 0)),
      );
    return interleave(groups, limit);
  });
  return interleave(groups, limit);
}
