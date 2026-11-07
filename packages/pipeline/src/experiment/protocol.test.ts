import { describe, expect, it } from 'vitest';
import * as pipeline from '../index';
import { createModelSettings } from '@proteus-int/intelligence';
import { createProtocol, reviewProtocol } from './protocol';

describe('balanced experiment protocol', () => {
  it('rotates all three participants through every position without parallel execution', () => {
    expect(pipeline.buildTrialOrder).toBeTypeOf('function');
    expect(pipeline.buildTrialOrder(3)).toEqual([
      ['a', 'b', 'neutral'],
      ['b', 'neutral', 'a'],
      ['neutral', 'a', 'b'],
    ]);
    expect(pipeline.buildTrialOrder(1)).toEqual([['a', 'b', 'neutral']]);
  });
});

it.each(['caller cancellation', { reason: 'caller cancellation' }])(
  'preserves arbitrary AbortSignal cancellation reasons during protocol review',
  async (reason) => {
    const protocol = createProtocol(
      'Task',
      createModelSettings(),
      [[], []],
      1,
      [['a', 'b', 'neutral']],
      {
        version: 2,
        criteria: [
          {
            id: 'complete',
            label: 'Complete',
            weight: 1,
            required: true,
            anchors: { unmet: 'No', partial: 'Partly', met: 'Yes' },
          },
        ],
      },
    );
    const controller = new AbortController();
    let finish: (() => void) | undefined;
    const pending = reviewProtocol(protocol, {
      signal: controller.signal,
      onProtocol: () =>
        new Promise<void>((resolve) => {
          finish = resolve;
        }),
    });
    await Promise.resolve();
    controller.abort(reason);
    await expect(pending).rejects.toBe(reason);
    finish?.();
  },
);
