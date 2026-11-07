import type { ReasoningBlock } from '@proteus-int/contracts';
import type { StageTrace } from '@proteus-int/pipeline';

export function mergeLiveReasoning(
  previous: StageTrace,
  reasoningBlock: ReasoningBlock,
  elapsedMs: number,
  callIndex = previous.attempts.length + 1,
): StageTrace {
  const block = { ...reasoningBlock, id: `${callIndex}:${reasoningBlock.id}` };
  const blocks = previous.trace.reasoning ?? [];
  const reasoning = blocks.some((item) => item.id === block.id)
    ? blocks.map((item) => (item.id === block.id ? block : item))
    : [...blocks, block];
  const wallTimeMs =
    previous.attempts
      .slice(0, callIndex - 1)
      .reduce((sum, attempt) => sum + attempt.wallTimeMs, 0) + elapsedMs;
  return {
    ...previous,
    trace: { ...previous.trace, reasoningMs: wallTimeMs, wallTimeMs, reasoning },
  };
}
