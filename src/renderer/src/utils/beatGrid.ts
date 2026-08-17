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
