import { describe, it, expect } from 'vitest';
import { mixerParams, type MixerParamSpec } from '@renderer/utils/sessionCore';
import { knobOrigin, knobTravel } from '@renderer/utils/knob';

const MIXERS = ['classic-3band', 'classic-4band'] as const;

function eqBands(mixerId: string): MixerParamSpec[] {
  return Object.values(mixerParams(mixerId)).filter((spec) => spec.slot === 'eq');
}

describe('eq band layout', () => {
  it.each(MIXERS)('every band on %s rests at twelve o clock', (mixerId) => {
    const bands = eqBands(mixerId);
    expect(bands.length).toBeGreaterThan(0);
    for (const spec of bands) {
      expect(knobTravel(spec, spec.defaultValue), `${mixerId} ${spec.param}`).toBe(0.5);
    }
  });

  it.each(MIXERS)('every band on %s lights its arc from the rest point', (mixerId) => {
    for (const spec of eqBands(mixerId)) {
      expect(knobOrigin(spec), `${mixerId} ${spec.param}`).toBe(
        knobTravel(spec, spec.defaultValue)
      );
    }
  });

  it.each(MIXERS)('every band on %s shares one range and step', (mixerId) => {
    const bands = eqBands(mixerId);
    const shape = ({ min, max, step, defaultValue }: MixerParamSpec) => ({
      min,
      max,
      step,
      defaultValue
    });
    for (const spec of bands) {
      expect(shape(spec), `${mixerId} ${spec.param}`).toEqual(shape(bands[0]));
    }
  });

  it.each(MIXERS)('%s boosts less than it cuts', (mixerId) => {
    const [spec] = eqBands(mixerId);
    expect(spec.max).toBeLessThan(Math.abs(spec.min));
  });
});
