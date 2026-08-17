import { describe, it, expect } from 'vitest';
import { beatLineStep, beatTier } from '../beatGrid';

describe('beatLineStep', () => {
  it('keeps step at 1 when beats are already spaced past the minimum', () => {
    expect(beatLineStep(10, 6, 4)).toBe(1);
  });

  it('escalates to the next group when beats are too close together', () => {
    // 2px/beat, needs 6px min: step 1 -> 2 (too close), step 4 -> 8 (clears it)
    expect(beatLineStep(2, 6, 4)).toBe(4);
  });

  it('escalates through multiple groups when beats are very dense', () => {
    // 0.5px/beat: step 1 -> 0.5, step 4 -> 2, step 16 -> 8 (clears 6px min)
    expect(beatLineStep(0.5, 6, 4)).toBe(16);
  });

  it('never returns a step that leaves lines under the minimum spacing', () => {
    const pxPerBeat = 0.3;
    const minSpacingPx = 8;
    const beatsPerGroup = 4;
    const step = beatLineStep(pxPerBeat, minSpacingPx, beatsPerGroup);
    expect(pxPerBeat * step).toBeGreaterThanOrEqual(minSpacingPx);
  });

  it('supports a non-power-of-4 group size', () => {
    // 1px/beat, needs 20px min: step 1 -> 1, step 3 -> 3, step 9 -> 9, step 27 -> 27
    expect(beatLineStep(1, 20, 3)).toBe(27);
  });
});

describe('beatTier', () => {
  it('classifies every 16th beat as a phrase', () => {
    expect(beatTier(0, 4, 16)).toBe('phrase');
    expect(beatTier(16, 4, 16)).toBe('phrase');
    expect(beatTier(32, 4, 16)).toBe('phrase');
  });

  it('classifies negative phrase-aligned beats the same as positive ones', () => {
    expect(beatTier(-16, 4, 16)).toBe('phrase');
  });

  it('classifies a non-phrase multiple of the bar size as a bar', () => {
    expect(beatTier(4, 4, 16)).toBe('bar');
    expect(beatTier(12, 4, 16)).toBe('bar');
  });

  it('classifies everything else as a plain beat', () => {
    expect(beatTier(1, 4, 16)).toBe('beat');
    expect(beatTier(3, 4, 16)).toBe('beat');
    expect(beatTier(5, 4, 16)).toBe('beat');
  });
});
