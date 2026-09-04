import { describe, it, expect } from 'vitest';
import { beatGridStep, beatLineStep, beatMarkerKind, visibleBeats } from '../beatGrid';
import { DEFAULT_METER, type Meter } from '../types';

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

describe('visibleBeats', () => {
  it('marks every fourth beat a downbeat and the rest not', () => {
    const beats = visibleBeats(120, 0, 0, 4, 1, DEFAULT_METER);
    expect(beats.map((beat) => beat.beatNumber)).toEqual([0, 1, 2, 3, 4, 5, 6, 7, 8]);
    expect(beats.filter((beat) => beat.isDownbeat).map((beat) => beat.beatNumber)).toEqual([
      0, 4, 8
    ]);
  });

  it('places beats a beat period apart from the offset', () => {
    const beats = visibleBeats(120, 0.25, 0, 1.5, 1, DEFAULT_METER);
    expect(beats.map((beat) => beat.sec)).toEqual([0.25, 0.75, 1.25]);
  });

  it('keeps only multiples of the step so the LOD thins the grid', () => {
    const beats = visibleBeats(120, 0, 0, 8, 4, DEFAULT_METER);
    expect(beats.map((beat) => beat.beatNumber)).toEqual([0, 4, 8, 12, 16]);
    expect(beats.every((beat) => beat.isDownbeat)).toBe(true);
  });

  it('walks backwards past the track start without losing downbeat alignment', () => {
    const beats = visibleBeats(120, 2, 0, 2, 1, DEFAULT_METER);
    expect(beats.map((beat) => beat.beatNumber)).toEqual([-4, -3, -2, -1, 0]);
    expect(beats.filter((beat) => beat.isDownbeat).map((beat) => beat.beatNumber)).toEqual([-4, 0]);
  });

  it('returns nothing for an unusable grid', () => {
    expect(visibleBeats(0, 0, 0, 10, 1, DEFAULT_METER)).toEqual([]);
    expect(visibleBeats(120, 0, 10, 0, 1, DEFAULT_METER)).toEqual([]);
    expect(visibleBeats(120, 0, 0, 10, 0, DEFAULT_METER)).toEqual([]);
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

describe('beatMarkerKind', () => {
  const at = (beatNumber: number) => ({
    beatNumber,
    sec: 0,
    isDownbeat: beatNumber % 4 === 0
  });

  it('separates a phrase start from a plain bar start', () => {
    expect(beatMarkerKind(at(32), DEFAULT_METER)).toBe('phrase');
    expect(beatMarkerKind(at(20), DEFAULT_METER)).toBe('bar');
    expect(beatMarkerKind(at(21), DEFAULT_METER)).toBe('beat');
  });

  it('keeps naming phrases before the beat grid origin', () => {
    expect(beatMarkerKind(at(-16), DEFAULT_METER)).toBe('phrase');
    expect(beatMarkerKind(at(-4), DEFAULT_METER)).toBe('bar');
  });

  it('names every line a phrase once the step reaches one', () => {
    const beats = visibleBeats(120, 0, 0, 60, 16, DEFAULT_METER);
    expect(beats.length).toBeGreaterThan(1);
    expect(beats.every((beat) => beatMarkerKind(beat, DEFAULT_METER) === 'phrase')).toBe(true);
  });
});

describe('a meter other than 4/4', () => {
  const waltz: Meter = { beatsPerBar: 3, barsPerPhrase: 4 };

  it('puts the downbeat on the meter, not on every fourth beat', () => {
    const beats = visibleBeats(120, 0, 0, 3, 1, waltz);
    expect(beats.filter((beat) => beat.isDownbeat).map((beat) => beat.beatNumber)).toEqual([
      0, 3, 6
    ]);
  });

  it('counts a phrase in bars, so it is twelve beats and not sixteen', () => {
    const at = (beatNumber: number) => ({
      beatNumber,
      sec: 0,
      isDownbeat: beatNumber % waltz.beatsPerBar === 0
    });
    expect(beatMarkerKind(at(12), waltz)).toBe('phrase');
    expect(beatMarkerKind(at(16), waltz)).toBe('beat');
  });
});
