import type { Meter } from '@renderer/utils/types';

export function beatLineStep(
  pxPerBeat: number,
  minSpacingPx: number,
  beatsPerGroup: number
): number {
  if (!(pxPerBeat > 0) || !(beatsPerGroup > 1)) return 1;
  let step = 1;
  while (pxPerBeat * step < minSpacingPx) step *= beatsPerGroup;
  return step;
}

export type VisibleBeat = { beatNumber: number; sec: number; isDownbeat: boolean };

export function visibleBeats(
  bpm: number,
  beatOffset: number,
  fromSec: number,
  toSec: number,
  step: number,
  meter: Meter
): VisibleBeat[] {
  if (bpm <= 0 || step <= 0 || toSec < fromSec) return [];
  const beatPeriod = 60 / bpm;
  const first = Math.ceil((fromSec - beatOffset) / beatPeriod);
  const last = Math.floor((toSec - beatOffset) / beatPeriod);

  const beats: VisibleBeat[] = [];
  for (let beatNumber = first; beatNumber <= last; beatNumber++) {
    if (beatNumber % step !== 0) continue;
    beats.push({
      beatNumber,
      sec: beatOffset + beatNumber * beatPeriod,
      isDownbeat: beatNumber % meter.beatsPerBar === 0
    });
  }
  return beats;
}

export type BeatMarkerKind = 'beat' | 'bar' | 'phrase';

export function beatMarkerKind(beat: VisibleBeat, meter: Meter): BeatMarkerKind {
  if (!beat.isDownbeat) return 'beat';
  const beatsPerPhrase = meter.beatsPerBar * meter.barsPerPhrase;
  return beat.beatNumber % beatsPerPhrase === 0 ? 'phrase' : 'bar';
}

// Once beats no longer fit, every surviving line is a downbeat and gets the heavy
// treatment, so the spacing that has to be met is the heavy one, not the hairline one.
export function beatGridStep(
  pxPerBeat: number,
  beatsPerGroup: number,
  minBeatSpacingPx: number,
  minBarSpacingPx: number
): number {
  const minimum = pxPerBeat < minBeatSpacingPx ? minBarSpacingPx : minBeatSpacingPx;
  return beatLineStep(pxPerBeat, minimum, beatsPerGroup);
}
