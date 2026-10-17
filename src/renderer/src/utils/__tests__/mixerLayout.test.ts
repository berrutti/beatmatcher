import { describe, it, expect } from 'vitest';
import { eqBandsInLayoutOrder } from '@renderer/utils/mixerLayout';
import { mixerParams, type MixerParamSpec } from '@renderer/utils/sessionCore';

function params(specs: MixerParamSpec[]): string[] {
  return specs.map((spec) => spec.param);
}

describe('mixer layout', () => {
  it('stacks the four-band EQ with the highs on top', () => {
    const eq = Object.values(mixerParams('classic-4band')).filter((spec) => spec.slot === 'eq');
    expect(params(eqBandsInLayoutOrder(eq))).toEqual(['high', 'highMid', 'lowMid', 'low']);
  });

  it('stacks the three-band EQ with the highs on top', () => {
    const eq = Object.values(mixerParams('classic-3band')).filter((spec) => spec.slot === 'eq');
    expect(params(eqBandsInLayoutOrder(eq))).toEqual(['high', 'highMid', 'low']);
  });

  it('still renders a control the layout does not name, after the named ones', () => {
    const [low] = Object.values(mixerParams('classic-4band')).filter(
      (spec) => spec.param === 'low'
    );
    const unnamed = { ...low, param: 'presence' };
    expect(params(eqBandsInLayoutOrder([unnamed, low]))).toEqual(['low', 'presence']);
  });
});
