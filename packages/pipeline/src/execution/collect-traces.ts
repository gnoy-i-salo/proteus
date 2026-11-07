import type { IntelligenceTrace } from '@proteus-int/contracts';
import type { StageCallbacks, StageTrace } from './types';

export function duration(trace: IntelligenceTrace) {
  return trace.wallTimeMs ?? trace.reasoningMs;
}

export function collectTraces(options: Pick<StageCallbacks, 'onTrace'>) {
  const traces: StageTrace[] = [];
  const notify = (entry: StageTrace) => {
    const index = traces.findIndex((item) => item.id === entry.id);
    if (index < 0) traces.push(entry);
    else traces[index] = entry;
    options.onTrace?.(structuredClone(entry));
  };
  return { traces, notify };
}
