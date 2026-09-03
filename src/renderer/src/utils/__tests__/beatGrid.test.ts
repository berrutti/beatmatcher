import { describe, it, expect } from 'vitest';
import { beatGridStep, beatLineStep, beatTier, visibleBeats } from '../beatGrid';

describe('beatLineStep', () => {
  it('keeps step at 1 when beats are already spaced past the minimum', () => {
    expect(beatLineStep(10, 6, 4)).toBe(1);
  });

  it('escalates to the next group when beats are too close together', () => {
    expect(beatLineStep(2, 6, 4)).toBe(4);
  });

  it('escalates through multiple groups when beats are very dense', () => {
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

describe('visibleBeats', () => {
  it('marks every fourth beat a downbeat and the rest not', () => {
    const beats = visibleBeats(120, 0, 0, 4, 1, 4);
    expect(beats.map((beat) => beat.beatNumber)).toEqual([0, 1, 2, 3, 4, 5, 6, 7, 8]);
    expect(beats.filter((beat) => beat.isDownbeat).map((beat) => beat.beatNumber)).toEqual([
      0, 4, 8
    ]);
  });

  it('places beats a beat period apart from the offset', () => {
    const beats = visibleBeats(120, 0.25, 0, 1.5, 1, 4);
    expect(beats.map((beat) => beat.sec)).toEqual([0.25, 0.75, 1.25]);
  });

  it('keeps only multiples of the step so the LOD thins the grid', () => {
    const beats = visibleBeats(120, 0, 0, 8, 4, 4);
    expect(beats.map((beat) => beat.beatNumber)).toEqual([0, 4, 8, 12, 16]);
    expect(beats.every((beat) => beat.isDownbeat)).toBe(true);
  });

  it('walks backwards past the track start without losing downbeat alignment', () => {
    const beats = visibleBeats(120, 2, 0, 2, 1, 4);
    expect(beats.map((beat) => beat.beatNumber)).toEqual([-4, -3, -2, -1, 0]);
    expect(beats.filter((beat) => beat.isDownbeat).map((beat) => beat.beatNumber)).toEqual([-4, 0]);
  });

  it('returns nothing for an unusable grid', () => {
    expect(visibleBeats(0, 0, 0, 10, 1, 4)).toEqual([]);
    expect(visibleBeats(120, 0, 10, 0, 1, 4)).toEqual([]);
    expect(visibleBeats(120, 0, 0, 10, 0, 4)).toEqual([]);
  });
});

describe('beatGridStep', () => {
  it('keeps every beat while beats still fit', () => {
    expect(beatGridStep(32, 4, 6, 24)).toBe(1);
    expect(beatGridStep(9, 4, 6, 24)).toBe(1);
  });

  it('jumps to bar spacing the moment beats stop fitting', () => {
    expect(beatGridStep(5, 4, 6, 24)).toBe(16);
    expect(beatGridStep(0.625, 4, 6, 24)).toBe(64);
  });

  it('never leaves a heavy line under the bar spacing', () => {
    for (const pxPerBeat of [0.2, 0.625, 1, 3.1, 5.9]) {
      expect(pxPerBeat * beatGridStep(pxPerBeat, 4, 6, 24)).toBeGreaterThanOrEqual(24);
    }
  });
});
