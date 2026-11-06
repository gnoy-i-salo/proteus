import { describe, expect, it } from 'vitest';
import * as pipeline from './index';

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
