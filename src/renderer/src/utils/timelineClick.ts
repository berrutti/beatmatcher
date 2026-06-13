// Decides what a (non-drag) click on the timeline canvas does. Scrubbing must
// stay possible everywhere in edit mode: every click in the track area seeks,
// and selection effects ride along. Clicking the waveform band also closes the
// open lane, so the band doubles as the "close lanes" surface.
export type ClickContext = {
  editMode: boolean;
  inLabelColumn: boolean;
  target: 'clip' | 'clip-band' | 'lane' | 'background';
  laneAlreadySelected?: boolean;
};

export type ClickPlan = {
  seek: boolean;
  selectClip: boolean;
  clearClipSelection: boolean;
  selectLane: boolean;
  clearLaneSelection: boolean;
};

const NOTHING: ClickPlan = {
  seek: false,
  selectClip: false,
  clearClipSelection: false,
  selectLane: false,
  clearLaneSelection: false
};

export function planTimelineClick(ctx: ClickContext): ClickPlan {
  if (!ctx.editMode) return { ...NOTHING, seek: true };

  if (ctx.inLabelColumn) {
    if (ctx.target !== 'lane') return NOTHING;
    // Deselecting a lane leaves nothing selected, so it does not also clear a
    // clip selection; selecting a lane does (lane and clip are distinct edit
    // targets and only one is active at a time).
    return ctx.laneAlreadySelected
      ? { ...NOTHING, clearLaneSelection: true }
      : { ...NOTHING, selectLane: true, clearClipSelection: true };
  }

  switch (ctx.target) {
    case 'clip':
      return { ...NOTHING, seek: true, selectClip: true, clearLaneSelection: true };
    case 'clip-band':
      return { ...NOTHING, seek: true, clearClipSelection: true, clearLaneSelection: true };
    case 'lane':
      return ctx.laneAlreadySelected
        ? { ...NOTHING, seek: true, clearClipSelection: true }
        : { ...NOTHING, seek: true, selectLane: true, clearClipSelection: true };
    case 'background':
      return { ...NOTHING, seek: true };
  }
}
