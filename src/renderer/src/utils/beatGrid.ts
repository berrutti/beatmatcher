// Shared by every beat-grid drawer (mixer/Waveform.vue, EditWaveform.vue,
// timelineDraw.ts's drawClipBeatGrid), which each computed this same
// escalating-step LOD independently.

// Caller guarantees pxPerBeat > 0 (each drawer already skips the zero/negative
// case before reaching here), or this never terminates.
export function beatLineStep(
  pxPerBeat: number,
  minSpacingPx: number,
  beatsPerGroup: number
): number {
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
  beatsPerBar: number
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
      isDownbeat: beatNumber % beatsPerBar === 0
    });
  }
  return beats;
}

export type BeatTier = 'phrase' | 'bar' | 'beat';

export function beatTier(
  beatNumber: number,
  beatsPerBar: number,
  beatsPerPhrase: number
): BeatTier {
  if (beatNumber % beatsPerPhrase === 0) return 'phrase';
  if (beatNumber % beatsPerBar === 0) return 'bar';
  return 'beat';
}

// Once beats no longer fit, every surviving line is a downbeat and gets the heavy
// treatment, so the spacing that has to be met is the heavy one, not the hairline one.
export function beatGridStep(
  pxPerBeat: number,
  beatsPerBar: number,
  minBeatSpacingPx: number,
  minBarSpacingPx: number
): number {
  const minimum = pxPerBeat < minBeatSpacingPx ? minBarSpacingPx : minBeatSpacingPx;
  return beatLineStep(pxPerBeat, minimum, beatsPerBar);
}
