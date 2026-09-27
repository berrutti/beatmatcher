import { describe, it, expect } from 'vitest';
import { knobOrigin, knobTravel, valueAtTravel } from '@renderer/utils/knob';
import { masterParams, mixerParams, type MixerParamSpec } from '@renderer/utils/sessionCore';

const MIXER = 'classic-4band';

function spec(key: string): MixerParamSpec {
  const found = mixerParams(MIXER)[key] ?? masterParams(MIXER)[key];
  if (!found) throw new Error(`no ${key}`);
  return found;
}

const KNOB_SLOTS = new Set(['eq', 'filter', 'send', 'echo']);

const ALL = [...Object.values(mixerParams(MIXER)), ...Object.values(masterParams(MIXER))].filter(
  (candidate) => candidate.unit !== 'bool' && KNOB_SLOTS.has(candidate.slot)
);

describe('knob travel', () => {
  it.each(ALL.map((candidate) => [`${candidate.slot}/${candidate.param}`, candidate] as const))(
    '%s spans the whole throw',
    (_key, candidate) => {
      expect(knobTravel(candidate, candidate.min)).toBeCloseTo(0, 9);
      expect(knobTravel(candidate, candidate.max)).toBeCloseTo(1, 9);
      expect(valueAtTravel(candidate, 0)).toBeCloseTo(candidate.min, 6);
      expect(valueAtTravel(candidate, 1)).toBeCloseTo(candidate.max, 6);
    }
  );

  it.each(ALL.map((candidate) => [`${candidate.slot}/${candidate.param}`, candidate] as const))(
    '%s lands back on its default',
    (_key, candidate) => {
      const travel = knobTravel(candidate, candidate.defaultValue);
      expect(valueAtTravel(candidate, travel)).toBeCloseTo(candidate.defaultValue, 6);
    }
  );

  it('rests the filter at twelve o clock and lights it from there', () => {
    const filter = spec('filter/value');
    expect(knobTravel(filter, 0)).toBe(0.5);
    expect(knobOrigin(filter)).toBe(0.5);
  });

  it('lights a send from fully off', () => {
    expect(knobOrigin(spec('send/amount'))).toBe(0);
  });

  it('gives a short echo as much of the throw as a long one', () => {
    const time = spec('echo/time');
    const shortHalf = knobTravel(time, 100) - knobTravel(time, 50);
    const longHalf = knobTravel(time, 2000) - knobTravel(time, 1000);
    expect(shortHalf).toBeCloseTo(longHalf, 9);
  });

  it('snaps to the step', () => {
    const eq = spec('eq/low');
    const value = valueAtTravel(eq, 0.61);
    expect(Math.abs(value / eq.step - Math.round(value / eq.step))).toBeLessThan(1e-9);
  });

  it('clamps travel past either end', () => {
    const send = spec('send/amount');
    expect(valueAtTravel(send, -0.3)).toBe(send.min);
    expect(valueAtTravel(send, 1.4)).toBe(send.max);
  });
});
