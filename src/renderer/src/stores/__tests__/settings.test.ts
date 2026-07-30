import { describe, it, expect, vi } from 'vitest';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn().mockResolvedValue({})
}));

vi.mock('@tauri-apps/plugin-store', () => ({
  load: vi.fn().mockResolvedValue({})
}));

import { FADER_CURVE_OPTIONS } from '../settings';
import { faderCurvePlots } from '@renderer/utils/sessionCore';

// The settings screen looks each curve up by the name it stores, and the engine
// names them independently. A mismatch draws an empty box rather than failing.
describe('fader curve options', () => {
  it('names every curve the engine plots, and no others', () => {
    const plotted = Object.keys(faderCurvePlots(4)).sort();
    expect([...FADER_CURVE_OPTIONS].sort()).toEqual(plotted);
  });

  it('plots one more point than the sample count, spanning the whole throw', () => {
    const plots = faderCurvePlots(4);
    for (const curve of FADER_CURVE_OPTIONS) {
      expect(plots[curve]).toHaveLength(5);
      expect(plots[curve][0]).toBe(0);
      expect(plots[curve][4]).toBe(1);
    }
  });
});
