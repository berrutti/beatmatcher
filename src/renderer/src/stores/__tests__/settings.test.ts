import { describe, it, expect, vi } from 'vitest';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn().mockResolvedValue({})
}));

vi.mock('@tauri-apps/plugin-store', () => ({
  load: vi.fn().mockResolvedValue({})
}));

import { FADER_CURVE_OPTIONS } from '../settings';
import { faderCurveGain } from '@renderer/utils/sessionCore';

// The settings screen asks for each curve by the name it stores, and the engine
// names them independently. A mismatch silently draws a linear line.
describe('fader curve options', () => {
  it('names a curve the engine recognises, for every option', () => {
    const shapes = FADER_CURVE_OPTIONS.map((curve) => faderCurveGain(curve, 0.5));
    expect(new Set(shapes).size).toBe(FADER_CURVE_OPTIONS.length);
  });

  it('holds both ends of the throw on every curve', () => {
    for (const curve of FADER_CURVE_OPTIONS) {
      expect(faderCurveGain(curve, 0), curve).toBe(0);
      expect(faderCurveGain(curve, 1), curve).toBe(1);
    }
  });

  it('orders the curves from quietest to loudest across the throw', () => {
    const [exponential, linear, logarithmic] = FADER_CURVE_OPTIONS;
    for (let step = 1; step < 10; step++) {
      const position = step / 10;
      expect(faderCurveGain(exponential, position)).toBeLessThan(
        faderCurveGain(linear, position)
      );
      expect(faderCurveGain(linear, position)).toBeLessThan(
        faderCurveGain(logarithmic, position)
      );
    }
  });
});
