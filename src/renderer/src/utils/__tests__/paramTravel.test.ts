import { describe, it, expect } from 'vitest';
import {
  masterParams,
  mixerParams,
  paramAtTravel,
  paramTravel,
  type MixerParamSpec
} from '@renderer/utils/sessionCore';

const MIXER = 'classic-4band';

function spec(key: string): MixerParamSpec {
  const found = mixerParams(MIXER)[key] ?? masterParams(MIXER)[key];
  if (!found) throw new Error(`no ${key}`);
  return found;
}

const KNOB_SLOTS = new Set(['eq', 'filter', 'send', 'reverb']);

const ALL = [...Object.values(mixerParams(MIXER)), ...Object.values(masterParams(MIXER))].filter(
  (candidate) => candidate.unit !== 'bool' && KNOB_SLOTS.has(candidate.slot)
);

describe('param travel', () => {
  it.each(ALL.map((candidate) => [`${candidate.slot}/${candidate.param}`, candidate] as const))(
    '%s spans the whole throw',
    (_key, candidate) => {
      expect(paramTravel(candidate, candidate.min)).toBeCloseTo(0, 9);
      expect(paramTravel(candidate, candidate.max)).toBeCloseTo(1, 9);
      expect(paramAtTravel(candidate, 0)).toBeCloseTo(candidate.min, 6);
      expect(paramAtTravel(candidate, 1)).toBeCloseTo(candidate.max, 6);
    }
  );

  it.each(ALL.map((candidate) => [`${candidate.slot}/${candidate.param}`, candidate] as const))(
    '%s lands back on its default',
    (_key, candidate) => {
      const travel = paramTravel(candidate, candidate.defaultValue);
      expect(paramAtTravel(candidate, travel)).toBeCloseTo(candidate.defaultValue, 6);
    }
  );

  it.each(['filter/value', 'eq/low'])('rests %s at twelve o clock', (key) => {
    const setting = spec(key);
    expect(paramTravel(setting, setting.defaultValue)).toBe(0.5);
  });

  it('rests the reverb tail further round than a linear knob would', () => {
    const tail = spec('reverb/tail');
    const linear = (tail.defaultValue - tail.min) / (tail.max - tail.min);
    expect(paramTravel(tail, tail.defaultValue)).toBeGreaterThan(linear);
  });

  it('rests the send fully off', () => {
    const send = spec('send/amount');
    expect(paramTravel(send, send.defaultValue)).toBe(0);
  });

  it('snaps to the step', () => {
    const eq = spec('eq/low');
    const value = paramAtTravel(eq, 0.61);
    expect(Math.abs(value / eq.step - Math.round(value / eq.step))).toBeLessThan(1e-9);
  });

  it('clamps travel past either end', () => {
    const send = spec('send/amount');
    expect(paramAtTravel(send, -0.3)).toBe(send.min);
    expect(paramAtTravel(send, 1.4)).toBe(send.max);
  });
});
