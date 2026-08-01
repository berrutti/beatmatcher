import type {
  Clip,
  WaveSegment,
  LoadedSpan,
  DeckLanes,
  LanePoint,
  NudgeSpan,
  EditableLaneKey,
  MasterLaneKey,
  DeckId
} from '@renderer/utils/types';
import { DECK_ACCENTS, DECK_LANE_KEYS } from '@renderer/utils/types';
import { editConstants, laneSpecs, type LaneSpec } from '@renderer/utils/sessionCore';
import { formatMs } from '@renderer/utils/time';
import { beatLineStep } from '@renderer/utils/beatGrid';
import {
  overlapsRange,
  msToFrac,
  sliceVisiblePoints,
  chooseTickInterval,
  type ViewWindow
} from '@renderer/utils/timelineView';

// A waveform slice for one track: `amps` are RMS points spanning the track-time
// range [startSec, endSec]. Zoom-driven LOD refetches a tighter range at higher
// point density, so this is a region (not necessarily the whole track).
export type WaveformRegion = { startSec: number; endSec: number; amps: Float32Array };
// The top-level region is the high-detail (visible) slice; `base` is a coarse
// slice covering the track's whole used extent, kept loaded so panning/zooming
// to an as-yet-unfetched spot still shows something until the detail arrives.
export type TrackWaveform = WaveformRegion & { base?: WaveformRegion };

export const DECK_ORDER = ['A', 'B', 'C', 'D'] as const;
// Waveform/clip band height. Taller than the old 44 so the (now higher-res)
// waveform and beat grid are legible.
export const ROW_H = 80;
export const LABEL_W = 32;
export const TICK_H = 16;
export const PADDING = 12;
const SUBLANE_H = 16;
export const MASTER_ROW_H = SUBLANE_H * 2;
export const OVERVIEW_H = 22;
export const OVERVIEW_GAP = 4;

export type LaneKey = (typeof DECK_LANE_KEYS)[number];

const BAR_HALF_HEIGHT_FRACTION = 0.45;
const BEAT_LINE_COLOR = '#ffffff1f';
const BPM_LABEL_BASELINE_OFFSET_PX = 10;
const BPM_LABEL_LEFT_PAD_PX = 3;
const CANVAS_BG_COLOR = '#0a0a0a';
const CLIP_FILL_ALPHA = '55';
const CLIP_GHOST_FILL_ALPHA = '50';
const CLIP_GHOST_LABEL_RIGHT_MARGIN_PX = 60;
const CLIP_GHOST_LABEL_Y_OFFSET_PX = 12;
const CLIP_STROKE_ALPHA = 'cc';
const CLIP_WAVEFORM_BAR_ALPHA = 'aa';
const DECK_LABEL_INACTIVE_COLOR = '#555';
const DECK_ROW_ZEBRA_COLOR_EVEN = '#161616';
const DECK_ROW_ZEBRA_COLOR_ODD = '#131313';
const DOWNBEAT_LINE_COLOR = '#ffffff4d';
const EQ_BAND_COLORS_HIGH = '#3b82f6';
const EQ_BAND_COLORS_LOW = '#ef4444';
const EQ_BAND_COLORS_MID = '#eab308';
const FILTER_ACTIVE_FILL = '#ffffff10';
const FILTER_HPF_COLOR = '#fb923c';
const FILTER_LPF_COLOR = '#38bdf8';
const FILTER_NEUTRAL_COLOR = '#666666';
const FRAME_GUTTER_COLOR = '#2a2a2a';
const GAIN_COLOR = '#e5e5e5';
const GESTURE_LABEL_CURSOR_GAP_PX = 8;
const GESTURE_PREVIEW_LINE_COLOR = '#ffffffcc';
const GESTURE_PREVIEW_LINE_WIDTH = 1.5;
const LABEL_FONT = '9px monospace';
const BOLD_LABEL_FONT = 'bold 9px monospace';
const SUB_LABEL_FONT = '7px monospace';
const BOLD_SUB_LABEL_FONT = 'bold 7px monospace';
const LABEL_OUTLINE_LINE_WIDTH = 3;
const LANE_BORDER_COLOR_GROUP_CHANGE = '#2a2a2a';
const LANE_BORDER_COLOR_SAME_GROUP = '#2e2e2e';
const LANE_CARET_OFFSET_PX = 6;
const LANE_CENTER_LINE_COLOR = '#4a4a4a';
const LANE_CENTER_LINE_DASH: [number, number] = [4, 4];
const LANE_GROUP_BG_COLOR_EVEN = '#1a1a1a';
const LANE_GROUP_BG_COLOR_ODD = '#141414';
const LANE_LABEL_OFFSET_PX = 5;
const LANE_VALUE_PAD_FRACTION = 4;
const LANE_VALUE_PAD_MAX_PX = 8;
const LOADED_SPAN_FILL_ALPHA = '18';
const LOADED_SPAN_STROKE_ALPHA = '40';
const MIN_DRAWABLE_CLIP_WIDTH_PX = 2;
const MIN_DRAWABLE_SEG_WIDTH_PX = 1;
const NUDGE_PREVIEW_LABEL_NEGATIVE_Y_OFFSET_PX = 6;
const NUDGE_PREVIEW_LABEL_POSITIVE_Y_OFFSET_PX = 12;
const NUDGE_PREVIEW_LABEL_RIGHT_MARGIN_PX = 30;
// Tighter inset than the deck lanes' laneValuePad, sized for the short master row.
export const MASTER_GAIN_INSET_Y = 2;
const MASTER_ROW_BG_COLOR = '#101010';
const LANE_DROPDOWN_COLOR = '#06b6d4';
const MASTER_LABEL_COLOR = '#888';
const MIN_BEAT_SPACING_PX = 8;
const BEATS_PER_BAR = 4;
const BEAT_LINE_W = 1;
// Vertical breathing room above and below clip bands, loaded spans, and clip
// selection boxes within the waveform strip.
const CLIP_BAND_INSET_Y = 4;
// A region narrower than this can't fit a "138.0" BPM label legibly, so it's
// skipped until the user zooms in enough to widen it.
const BPM_LABEL_MIN_PX = 30;
const MUTE_COLOR = '#ef4444';
const NUDGE_COLOR = '#fbbf24';
const NUDGE_LINE_W = 2;
const OVERVIEW_BORDER_COLOR = '#222';
const OVERVIEW_CLIP_ALPHA = 'aa';
const OVERVIEW_PLAYHEAD_COLOR = '#ffffffcc';
const OVERVIEW_VIEWPORT_FILL_COLOR = '#ffffff15';
const OVERVIEW_VIEWPORT_STROKE_COLOR = '#ffffff80';
const PAINT_PREVIEW_LABEL_RIGHT_MARGIN_PX = 30;
const PAINT_PREVIEW_LABEL_Y_OFFSET_PX = 6;
const PAINT_PREVIEW_OFF_COLOR = '#00000060';
const PAINT_PREVIEW_ON_COLOR = '#ffffff30';
const PLAYHEAD_ALPHA = 0.9;
const PLAYHEAD_COLOR = '#ffffff';
const PLAYHEAD_LINE_WIDTH = 1.5;
const RATE_COLOR = '#a78bfa';
// So an endFrac landing exactly on an integer index (float rounding) still
// truncates to the previous index, keeping sampleRegion's range exclusive.
const SAMPLE_END_INDEX_EPSILON = 1e-9;
const SELECTION_FILL_COLOR = 'rgba(255, 255, 255, 0.14)';
const SELECTION_STROKE_COLOR = 'rgba(255, 255, 255, 0.9)';
const SOLO_COLOR = '#eab308';
const SOLO_MUTE_LABEL_OFFSET_PX = 11;
const SPAN_LABEL_INSET_PX = 3;
const SPAN_LABEL_MIN_PX = 40;
const TEXT_FILL_COLOR = '#ffffff';
const TEXT_OUTLINE_COLOR = '#000000cc';
const TICK_LABEL_COLOR = '#555';
const TICK_MARK_COLOR = '#333';
const TICK_ROW_BG_COLOR = '#161616';
const VALUE_PREVIEW_LABEL_RIGHT_MARGIN_PX = 40;
const VALUE_PREVIEW_LABEL_Y_OFFSET_PX = 6;

// Row divider: a dark gap topped by a bright hairline, heavier than the 1px
// sublane-group separators so deck/master boundaries stand out.
const ROW_DIVIDER_H = 3;
const ROW_DIVIDER_LINE_H = 1;
const ROW_DIVIDER_GAP_COLOR = '#000';
const ROW_DIVIDER_LINE_COLOR = '#5a5a5a';

export type SublaneLayout = { key: LaneKey; top: number; height: number };
export type RowLayout = {
  deckId: DeckId;
  top: number;
  height: number;
  // Height of the waveform strip at the top of the row (resizable, see ROW_H).
  waveformHeight: number;
  lanes: SublaneLayout[];
};
export type OverviewRect = { y: number; h: number };

function formatTickLabel(ms: number, tickIntervalMs: number): string {
  if (tickIntervalMs < 1000) {
    const totalSec = Math.floor(ms / 1000);
    const mins = Math.floor(totalSec / 60);
    const secs = totalSec % 60;
    const millis = ms % 1000;
    return `${mins}:${String(secs).padStart(2, '0')}:${String(millis).padStart(3, '0')}`;
  }
  return formatMs(ms);
}

function valueToY(
  laneY: number,
  laneHeight: number,
  minVal: number,
  maxVal: number,
  value: number
): number {
  const range = maxVal - minVal || 1;
  return laneY + laneHeight - ((value - minVal) / range) * laneHeight;
}

// Vertical breathing room between the lane frame and its value area. Shared by
// the renderer and the draw gesture so what you draw lands where it's rendered.
export function laneValuePad(height: number): number {
  return Math.min(LANE_VALUE_PAD_MAX_PX, height / LANE_VALUE_PAD_FRACTION);
}

export function yToValue(
  laneY: number,
  laneHeight: number,
  minVal: number,
  maxVal: number,
  y: number
): number {
  const range = maxVal - minVal || 1;
  const value = minVal + ((laneY + laneHeight - y) / (laneHeight || 1)) * range;
  return Math.min(maxVal, Math.max(minVal, value));
}

function filterColorFor(value: number): string {
  const { filterDeadZone } = editConstants();
  if (value < -filterDeadZone) return FILTER_LPF_COLOR;
  if (value > filterDeadZone) return FILTER_HPF_COLOR;
  return FILTER_NEUTRAL_COLOR;
}

// Each clip is drawn as the concatenation of its wave segments, every segment
// mapping a track-time window to a wall-time window at its own effective rate
// (pitch*nudge). That is what makes a region the user nudged/pitched render
// longer or shorter, instead of the whole clip at one wrong rate.
function clipWaveSegments(clip: Clip): WaveSegment[] {
  if (clip.waveSegments.length > 0) return clip.waveSegments;
  // Legacy fallback for clips with no segments: one piece at the nominal rate.
  const trackEndSec =
    clip.trackStartSec + ((clip.sessionEndMs - clip.sessionStartMs) / 1000) * clip.playbackRate;
  return [
    {
      wallStartMs: clip.sessionStartMs,
      wallEndMs: clip.sessionEndMs,
      trackStartSec: clip.trackStartSec,
      trackEndSec
    }
  ];
}

// Mean amplitude over the track-time range [t0, t1] within a region, or null if
// that range lies outside the region (so the caller can fall back to a coarser one).
function sampleRegion(region: WaveformRegion, startSec: number, endSec: number): number | null {
  const span = region.endSec - region.startSec;
  if (span <= 0) return null;
  const startFrac = (startSec - region.startSec) / span;
  const endFrac = (endSec - region.startSec) / span;
  if (endFrac <= 0 || startFrac >= 1) return null;
  const sampleCount = region.amps.length;
  const startIdx = Math.min(sampleCount - 1, Math.max(0, (startFrac * sampleCount) | 0));
  const endIdx = Math.min(
    sampleCount - 1,
    Math.max(startIdx, (endFrac * sampleCount - SAMPLE_END_INDEX_EPSILON) | 0)
  );
  let sum = 0;
  for (let idx = startIdx; idx <= endIdx; idx++) sum += region.amps[idx];
  return sum / (endIdx - startIdx + 1);
}

function drawClipWaveform(
  ctx: CanvasRenderingContext2D,
  clip: Clip,
  waveform: TrackWaveform | undefined,
  rectY: number,
  rectHeight: number,
  accent: string,
  msToX: (ms: number) => number
): void {
  if (!waveform || waveform.amps.length === 0) return;

  const clipX0 = msToX(clip.sessionStartMs);
  const clipX1 = msToX(clip.sessionEndMs);
  if (clipX1 - clipX0 < MIN_DRAWABLE_CLIP_WIDTH_PX) return;

  const centerY = rectY + rectHeight / 2;
  const maxBarHalf = rectHeight * BAR_HALF_HEIGHT_FRACTION;
  const base = waveform.base;

  ctx.save();
  ctx.beginPath();
  ctx.rect(clipX0, rectY, clipX1 - clipX0, rectHeight);
  ctx.clip();
  ctx.fillStyle = accent + CLIP_WAVEFORM_BAR_ALPHA;

  // Only draw columns inside the visible track area: when zoomed in a clip can
  // be tens of thousands of pixels wide and drawing offscreen columns froze the UI.
  const visibleLeft = Math.max(LABEL_W, clipX0);
  const visibleRight = Math.min(ctx.canvas.clientWidth - PADDING, clipX1);

  for (const seg of clipWaveSegments(clip)) {
    const segX0 = msToX(seg.wallStartMs);
    const segWidth = msToX(seg.wallEndMs) - segX0;
    if (segWidth < MIN_DRAWABLE_SEG_WIDTH_PX) continue;
    const segTrackSpan = seg.trackEndSec - seg.trackStartSec;

    const columnStart = Math.max(0, Math.floor(Math.max(visibleLeft, segX0) - segX0));
    const columnEnd = Math.ceil(Math.min(visibleRight, segX0 + segWidth) - segX0);
    for (let column = columnStart; column < columnEnd; column++) {
      // The track-time window this column covers, then sample the high-detail
      // region first and fall back to the coarse base while detail is loading.
      const columnStartSec = seg.trackStartSec + (column / segWidth) * segTrackSpan;
      const columnEndSec = seg.trackStartSec + ((column + 1) / segWidth) * segTrackSpan;
      const amp =
        sampleRegion(waveform, columnStartSec, columnEndSec) ??
        (base ? sampleRegion(base, columnStartSec, columnEndSec) : null);
      if (amp === null) continue;
      const barHeight = Math.max(1, Math.sqrt(amp) * maxBarHalf);
      ctx.fillRect(segX0 + column, centerY - barHeight, 1, barHeight * 2);
    }
  }

  ctx.restore();
}

// Beat grid over a clip, drawn per wave segment so the lines compress/stretch
// with the waveform when the rate changes. Ported from EditWaveform's LOD steps
// (1 -> 4 -> 16 -> ... beats), but mapped through each segment's track->wall rate.
function drawClipBeatGrid(
  ctx: CanvasRenderingContext2D,
  clip: Clip,
  rectY: number,
  rectHeight: number,
  msToX: (ms: number) => number
): void {
  if (!clip.bpm || clip.bpm <= 0) return;
  const beatDurSec = 60 / clip.bpm;
  const beatOffset = clip.beatOffsetSec ?? 0;

  const clipX0 = msToX(clip.sessionStartMs);
  const clipX1 = msToX(clip.sessionEndMs);
  ctx.save();
  ctx.beginPath();
  ctx.rect(clipX0, rectY, clipX1 - clipX0, rectHeight);
  ctx.clip();

  for (const seg of clipWaveSegments(clip)) {
    const segWallSec = (seg.wallEndMs - seg.wallStartMs) / 1000;
    const trackSpan = seg.trackEndSec - seg.trackStartSec;
    if (segWallSec <= 0 || trackSpan <= 0) continue;
    const effRate = trackSpan / segWallSec; // track sec per wall sec
    const segX0 = msToX(seg.wallStartMs);
    const pxPerWallSec = (msToX(seg.wallEndMs) - segX0) / segWallSec;
    const pxPerBeat = (beatDurSec / effRate) * pxPerWallSec;
    if (pxPerBeat < BEAT_LINE_W) continue;

    const step = beatLineStep(pxPerBeat, MIN_BEAT_SPACING_PX, BEATS_PER_BAR);

    const firstBeat = Math.ceil((seg.trackStartSec - beatOffset) / beatDurSec);
    const lastBeat = Math.floor((seg.trackEndSec - beatOffset) / beatDurSec);
    for (let beat = firstBeat; beat <= lastBeat; beat++) {
      if (beat % step !== 0) continue;
      const beatSec = beatOffset + beat * beatDurSec;
      const beatX = segX0 + ((beatSec - seg.trackStartSec) / effRate) * pxPerWallSec;
      ctx.fillStyle = beat % (step * BEATS_PER_BAR) === 0 ? DOWNBEAT_LINE_COLOR : BEAT_LINE_COLOR;
      ctx.fillRect(beatX, rectY, BEAT_LINE_W, rectHeight);
    }
  }

  ctx.restore();
}

// A BPM number at the start of each constant-rate region (wave segment), drawn
// like the track-name label. The shown BPM is the track grid bpm scaled by the
// segment's effective rate (track-sec per wall-sec), so each region the user
// pitched/nudged reads its actual tempo. Regions too narrow to fit the text are
// skipped, so rapid changes only reveal their numbers once zoomed in.
export function drawClipBpmLabels(
  ctx: CanvasRenderingContext2D,
  clip: Clip,
  rowY: number,
  rowH: number,
  msToX: (ms: number) => number
): void {
  if (!clip.bpm || clip.bpm <= 0) return;
  const clipY = rowY + CLIP_BAND_INSET_Y;
  const clipHeight = rowH - 2 * CLIP_BAND_INSET_Y;
  const clipX0 = msToX(clip.sessionStartMs);
  const clipX1 = msToX(clip.sessionEndMs);
  ctx.save();
  ctx.beginPath();
  ctx.rect(clipX0, clipY, clipX1 - clipX0, clipHeight);
  ctx.clip();
  for (const seg of clipWaveSegments(clip)) {
    const segX0 = msToX(seg.wallStartMs);
    const segWidth = msToX(seg.wallEndMs) - segX0;
    if (segWidth < BPM_LABEL_MIN_PX) continue;
    const segWallSec = (seg.wallEndMs - seg.wallStartMs) / 1000;
    const trackSpan = seg.trackEndSec - seg.trackStartSec;
    if (segWallSec <= 0 || trackSpan <= 0) continue;
    const bpm = clip.bpm * (trackSpan / segWallSec);
    const labelX = Math.max(LABEL_W + BPM_LABEL_LEFT_PAD_PX, segX0 + BPM_LABEL_LEFT_PAD_PX);
    drawOutlinedLabel(ctx, bpm.toFixed(1), labelX, clipY + BPM_LABEL_BASELINE_OFFSET_PX);
  }
  ctx.restore();
}

// Draws a zero-order-hold (step) graph: each point's value is held flat until
// the next point's time, then jumps. This matches how parameters actually change:
// session events fire at the moment a value changes, not gradually beforehand.
function drawLaneSteps(
  ctx: CanvasRenderingContext2D,
  points: LanePoint[],
  laneY: number,
  laneHeight: number,
  minVal: number,
  maxVal: number,
  color: string | ((value: number) => string),
  msToX: (ms: number) => number,
  viewStart: number,
  viewEnd: number
): void {
  const visible = sliceVisiblePoints(points, viewStart, viewEnd);
  if (visible.length < 2) return;
  const colorFor = typeof color === 'function' ? color : () => color;
  ctx.lineWidth = 1;
  for (let pointIdx = 0; pointIdx < visible.length - 1; pointIdx++) {
    const cur = visible[pointIdx];
    const next = visible[pointIdx + 1];
    const segX0 = msToX(cur.ms);
    const segX1 = msToX(next.ms);
    const stepY = valueToY(laneY, laneHeight, minVal, maxVal, cur.value);
    const nextStepY = valueToY(laneY, laneHeight, minVal, maxVal, next.value);

    ctx.strokeStyle = colorFor(cur.value);
    ctx.beginPath();
    ctx.moveTo(segX0, stepY);
    ctx.lineTo(segX1, stepY);
    ctx.stroke();

    if (nextStepY !== stepY) {
      ctx.strokeStyle = colorFor(next.value);
      ctx.beginPath();
      ctx.moveTo(segX1, stepY);
      ctx.lineTo(segX1, nextStepY);
      ctx.stroke();
    }
  }
}

function drawFilterLane(
  ctx: CanvasRenderingContext2D,
  canvasWidth: number,
  deckData: DeckLanes,
  laneY: number,
  laneH: number,
  msToX: (ms: number) => number,
  viewStart: number,
  viewEnd: number,
  specs: LaneSpecs
): void {
  for (const span of deckData.filterActive) {
    if (!overlapsRange(span.startMs, span.endMs, viewStart, viewEnd)) continue;
    const spanX = msToX(span.startMs);
    const spanWidth = Math.max(1, msToX(span.endMs) - spanX);
    ctx.fillStyle = FILTER_ACTIVE_FILL;
    ctx.fillRect(spanX, laneY, spanWidth, laneH);
  }

  const { min, max, defaultValue } = specs.filter;

  // Center line marks the bypass position (knob = 0); LPF sweeps below it, HPF above.
  drawLaneCenterLine(ctx, canvasWidth, laneY, laneH, min, max, defaultValue);

  drawLaneSteps(
    ctx,
    deckData.filter,
    laneY,
    laneH,
    min,
    max,
    filterColorFor,
    msToX,
    viewStart,
    viewEnd
  );
}

function drawLaneCenterLine(
  ctx: CanvasRenderingContext2D,
  canvasWidth: number,
  laneY: number,
  laneH: number,
  minVal: number,
  maxVal: number,
  centerValue: number
): void {
  const centerY = valueToY(laneY, laneH, minVal, maxVal, centerValue);
  ctx.save();
  ctx.strokeStyle = LANE_CENTER_LINE_COLOR;
  ctx.setLineDash(LANE_CENTER_LINE_DASH);
  ctx.beginPath();
  ctx.moveTo(LABEL_W, centerY + 0.5);
  ctx.lineTo(canvasWidth - PADDING, centerY + 0.5);
  ctx.stroke();
  ctx.restore();
}

function drawRateLane(
  ctx: CanvasRenderingContext2D,
  canvasWidth: number,
  deckData: DeckLanes,
  laneY: number,
  laneH: number,
  msToX: (ms: number) => number,
  viewStart: number,
  viewEnd: number
): void {
  // Center line marks the neutral rate (1.0 = 0% pitch).
  drawLaneCenterLine(ctx, canvasWidth, laneY, laneH, deckData.rateMin, deckData.rateMax, 1);

  drawLaneSteps(
    ctx,
    deckData.rate,
    laneY,
    laneH,
    deckData.rateMin,
    deckData.rateMax,
    RATE_COLOR,
    msToX,
    viewStart,
    viewEnd
  );
}

function drawSpecLane(
  key: LaneKey,
  ctx: CanvasRenderingContext2D,
  points: LanePoint[],
  color: string,
  laneY: number,
  laneH: number,
  msToX: (ms: number) => number,
  viewStart: number,
  viewEnd: number,
  specs: LaneSpecs
): void {
  const { min, max } = specs[key];
  drawLaneSteps(ctx, points, laneY, laneH, min, max, color, msToX, viewStart, viewEnd);
}

type LaneSpecs = Record<EditableLaneKey, LaneSpec>;

type LaneDrawer = (
  ctx: CanvasRenderingContext2D,
  canvasWidth: number,
  deckData: DeckLanes,
  laneY: number,
  laneH: number,
  msToX: (ms: number) => number,
  viewStart: number,
  viewEnd: number,
  specs: LaneSpecs
) => void;

const LANE_DRAWERS: Record<LaneKey, LaneDrawer> = {
  gain: (ctx, _canvasWidth, deckData, laneY, laneH, msToX, viewStart, viewEnd, specs) =>
    drawSpecLane(
      'gain',
      ctx,
      deckData.gain,
      GAIN_COLOR,
      laneY,
      laneH,
      msToX,
      viewStart,
      viewEnd,
      specs
    ),
  filter: drawFilterLane,
  rate: drawRateLane,
  eqLow: (ctx, _canvasWidth, deckData, laneY, laneH, msToX, viewStart, viewEnd, specs) =>
    drawSpecLane(
      'eqLow',
      ctx,
      deckData.eqLow,
      EQ_BAND_COLORS_LOW,
      laneY,
      laneH,
      msToX,
      viewStart,
      viewEnd,
      specs
    ),
  eqMid: (ctx, _canvasWidth, deckData, laneY, laneH, msToX, viewStart, viewEnd, specs) =>
    drawSpecLane(
      'eqMid',
      ctx,
      deckData.eqMid,
      EQ_BAND_COLORS_MID,
      laneY,
      laneH,
      msToX,
      viewStart,
      viewEnd,
      specs
    ),
  eqHigh: (ctx, _canvasWidth, deckData, laneY, laneH, msToX, viewStart, viewEnd, specs) =>
    drawSpecLane(
      'eqHigh',
      ctx,
      deckData.eqHigh,
      EQ_BAND_COLORS_HIGH,
      laneY,
      laneH,
      msToX,
      viewStart,
      viewEnd,
      specs
    )
};

// Clip drawing to one lane's rect. The single place lane-bounded drawing is
// contained, so strokes/outlines (committed lines, the in-progress gesture, the
// filter-span selection box, anything future) can't spill past the lane's
// dividers, instead of every drawer guarding its own edges.
function withLaneClip(
  ctx: CanvasRenderingContext2D,
  top: number,
  height: number,
  canvasWidth: number,
  draw: () => void
): void {
  ctx.save();
  ctx.beginPath();
  ctx.rect(LABEL_W, top, canvasWidth - LABEL_W - PADDING, height);
  ctx.clip();
  draw();
  ctx.restore();
}

export function drawDeckLanes(
  ctx: CanvasRenderingContext2D,
  canvasWidth: number,
  msToX: (ms: number) => number,
  deckData: DeckLanes | undefined,
  sublanes: SublaneLayout[],
  viewStart: number,
  viewEnd: number,
  mixerId: string
): void {
  const specs = laneSpecs(mixerId);
  for (let laneIdx = 0; laneIdx < sublanes.length; laneIdx++) {
    const { key, top, height } = sublanes[laneIdx];
    const group = specs[key].laneGroup;
    const prevGroup = laneIdx > 0 ? specs[sublanes[laneIdx - 1].key].laneGroup : -1;
    const trackW = canvasWidth - LABEL_W - PADDING;

    ctx.fillStyle = group % 2 === 0 ? LANE_GROUP_BG_COLOR_EVEN : LANE_GROUP_BG_COLOR_ODD;
    ctx.fillRect(LABEL_W, top, trackW, height);

    // Frame the lane with a top border so it reads as a bounded panel rather
    // than bleeding into the waveform above. Drawn regardless of data so the
    // separation is consistent whether or not the deck/lane has content.
    ctx.fillStyle =
      laneIdx > 0 && group !== prevGroup
        ? LANE_BORDER_COLOR_GROUP_CHANGE
        : LANE_BORDER_COLOR_SAME_GROUP;
    ctx.fillRect(LABEL_W, top, trackW, 1);

    // The value curve needs deck data; the frame above does not.
    if (!deckData) continue;

    // Inset the value area so the curve breathes and never touches the frame;
    // the center stays at the lane's exact middle, keeping the halves symmetric.
    const pad = laneValuePad(height);
    withLaneClip(ctx, top, height, canvasWidth, () =>
      LANE_DRAWERS[key](
        ctx,
        canvasWidth,
        deckData,
        top + pad,
        height - 2 * pad,
        msToX,
        viewStart,
        viewEnd,
        specs
      )
    );
  }
}

export function drawLoadedSpan(
  ctx: CanvasRenderingContext2D,
  span: LoadedSpan,
  rowY: number,
  rowH: number,
  accent: string,
  msToX: (ms: number) => number
): void {
  const spanX = msToX(span.startMs);
  const spanWidth = Math.max(2, msToX(span.endMs) - spanX);
  const spanY = rowY + CLIP_BAND_INSET_Y;
  const spanHeight = rowH - 2 * CLIP_BAND_INSET_Y;

  ctx.fillStyle = accent + LOADED_SPAN_FILL_ALPHA;
  ctx.fillRect(spanX, spanY, spanWidth, spanHeight);

  ctx.strokeStyle = accent + LOADED_SPAN_STROKE_ALPHA;
  ctx.lineWidth = 1;
  ctx.strokeRect(spanX + 0.5, spanY + 0.5, spanWidth - 1, spanHeight - 1);
}

export function drawLoadedSpanLabel(
  ctx: CanvasRenderingContext2D,
  span: LoadedSpan,
  rowY: number,
  rowH: number,
  msToX: (ms: number) => number
): void {
  const spanX = msToX(span.startMs);
  const spanWidth = Math.max(2, msToX(span.endMs) - spanX);
  if (spanWidth <= SPAN_LABEL_MIN_PX) return;
  const spanY = rowY + CLIP_BAND_INSET_Y;
  const spanHeight = rowH - 2 * CLIP_BAND_INSET_Y;
  ctx.save();
  ctx.beginPath();
  ctx.rect(spanX + SPAN_LABEL_INSET_PX, spanY, spanWidth - SPAN_LABEL_INSET_PX * 2, spanHeight);
  ctx.clip();
  ctx.font = LABEL_FONT;
  ctx.textAlign = 'left';
  ctx.textBaseline = 'middle';
  const textX = Math.max(LABEL_W + SPAN_LABEL_INSET_PX, spanX + SPAN_LABEL_INSET_PX);
  const textY = spanY + spanHeight / 2;
  ctx.lineWidth = LABEL_OUTLINE_LINE_WIDTH;
  ctx.strokeStyle = TEXT_OUTLINE_COLOR;
  ctx.lineJoin = 'round';
  ctx.strokeText(span.trackName, textX, textY);
  ctx.fillStyle = TEXT_FILL_COLOR;
  ctx.fillText(span.trackName, textX, textY);
  ctx.restore();
}

export function drawClip(
  ctx: CanvasRenderingContext2D,
  clip: Clip,
  waveform: TrackWaveform | undefined,
  rowY: number,
  rowH: number,
  accent: string,
  msToX: (ms: number) => number
): void {
  const clipX = msToX(clip.sessionStartMs);
  const clipWidth = Math.max(2, msToX(clip.sessionEndMs) - clipX);
  const clipY = rowY + CLIP_BAND_INSET_Y;
  const clipHeight = rowH - 2 * CLIP_BAND_INSET_Y;

  ctx.fillStyle = accent + CLIP_FILL_ALPHA;
  ctx.fillRect(clipX, clipY, clipWidth, clipHeight);

  drawClipWaveform(ctx, clip, waveform, clipY, clipHeight, accent, msToX);
  drawClipBeatGrid(ctx, clip, clipY, clipHeight, msToX);

  ctx.strokeStyle = accent + CLIP_STROKE_ALPHA;
  ctx.lineWidth = 1;
  ctx.strokeRect(clipX + 0.5, clipY + 0.5, clipWidth - 1, clipHeight - 1);
}

// White so it stays visible over any user-chosen deck accent color.
export function drawClipSelection(
  ctx: CanvasRenderingContext2D,
  startMs: number,
  endMs: number,
  rowY: number,
  rowH: number,
  msToX: (ms: number) => number
): void {
  const selectionX = msToX(startMs);
  const selectionWidth = Math.max(2, msToX(endMs) - selectionX);
  const selectionY = rowY + CLIP_BAND_INSET_Y;
  const selectionHeight = rowH - 2 * CLIP_BAND_INSET_Y;
  ctx.fillStyle = SELECTION_FILL_COLOR;
  ctx.fillRect(selectionX, selectionY, selectionWidth, selectionHeight);
  ctx.strokeStyle = SELECTION_STROKE_COLOR;
  ctx.lineWidth = 1;
  ctx.strokeRect(selectionX + 0.5, selectionY + 0.5, selectionWidth - 1, selectionHeight - 1);
}

export function drawNudgeSpans(
  ctx: CanvasRenderingContext2D,
  nudgeSpans: NudgeSpan[],
  rowY: number,
  rowH: number,
  msToX: (ms: number) => number
): void {
  if (nudgeSpans.length === 0) return;
  const innerY = rowY + CLIP_BAND_INSET_Y;
  const innerHeight = rowH - 2 * CLIP_BAND_INSET_Y;
  ctx.fillStyle = NUDGE_COLOR;
  for (const span of nudgeSpans) {
    const nudgeX = msToX(span.startMs);
    const nudgeWidth = Math.max(2, msToX(span.endMs) - nudgeX);
    const nudgeLineY = span.percent > 0 ? innerY - NUDGE_LINE_W : innerY + innerHeight;
    ctx.fillRect(nudgeX, nudgeLineY, nudgeWidth, NUDGE_LINE_W);
  }
}

export function drawOverview(
  ctx: CanvasRenderingContext2D,
  canvasWidth: number,
  trackWidth: number,
  stripY: number,
  totalMs: number,
  viewStart: number,
  viewEnd: number,
  clips: Clip[],
  playheadMs: number,
  accents: Record<string, string>
): OverviewRect {
  function fullMsToX(ms: number): number {
    return LABEL_W + (ms / totalMs) * trackWidth;
  }

  ctx.fillStyle = CANVAS_BG_COLOR;
  ctx.fillRect(0, stripY, canvasWidth, OVERVIEW_H);

  const laneHeight = (OVERVIEW_H - 4) / DECK_ORDER.length;
  for (let deckIdx = 0; deckIdx < DECK_ORDER.length; deckIdx++) {
    const deckId = DECK_ORDER[deckIdx];
    const accent = accents[deckId] ?? DECK_ACCENTS[deckId];
    const overviewLaneY = stripY + 2 + deckIdx * laneHeight;
    ctx.fillStyle = accent + OVERVIEW_CLIP_ALPHA;
    for (const clip of clips) {
      if (clip.deck !== deckId) continue;
      const clipX = fullMsToX(clip.sessionStartMs);
      const clipWidth = Math.max(1, fullMsToX(clip.sessionEndMs) - clipX);
      ctx.fillRect(clipX, overviewLaneY, clipWidth, Math.max(1, laneHeight - 1));
    }
  }

  if (playheadMs > 0) {
    ctx.fillStyle = OVERVIEW_PLAYHEAD_COLOR;
    ctx.fillRect(fullMsToX(playheadMs) - 0.5, stripY, 1, OVERVIEW_H);
  }

  const viewportX = fullMsToX(viewStart);
  const viewportWidth = Math.max(2, fullMsToX(viewEnd) - viewportX);
  ctx.fillStyle = OVERVIEW_VIEWPORT_FILL_COLOR;
  ctx.fillRect(viewportX, stripY, viewportWidth, OVERVIEW_H);
  ctx.strokeStyle = OVERVIEW_VIEWPORT_STROKE_COLOR;
  ctx.lineWidth = 1;
  ctx.strokeRect(viewportX + 0.5, stripY + 0.5, Math.max(1, viewportWidth - 1), OVERVIEW_H - 1);

  ctx.fillStyle = OVERVIEW_BORDER_COLOR;
  ctx.fillRect(0, stripY - 1, canvasWidth, 1);

  return { y: stripY, h: OVERVIEW_H };
}

export function drawMasterLane(
  ctx: CanvasRenderingContext2D,
  points: LanePoint[],
  lane: MasterLaneKey,
  masterTopY: number,
  masterRowH: number,
  canvasWidth: number,
  msToX: (ms: number) => number,
  viewStart: number,
  viewEnd: number,
  mixerId: string
): void {
  // Clip to the track area like the deck lanes do, so the level line never
  // bleeds left into the "M" label gutter or right into the padding.
  const { min, max } = laneSpecs(mixerId)[lane];
  withLaneClip(ctx, masterTopY, masterRowH, canvasWidth, () =>
    drawLaneSteps(
      ctx,
      points,
      masterTopY + MASTER_GAIN_INSET_Y,
      masterRowH - 2 * MASTER_GAIN_INSET_Y,
      min,
      max,
      GAIN_COLOR,
      msToX,
      viewStart,
      viewEnd
    )
  );
}

export function makeMsToX(view: ViewWindow, trackWidth: number): (ms: number) => number {
  return (ms: number) => LABEL_W + msToFrac(ms, view) * trackWidth;
}

// Renderers extracted from Timeline.vue's draw(): each takes the context plus
// explicit data, no component state, so draw() stays a short orchestrator.

function drawOutlinedLabel(
  ctx: CanvasRenderingContext2D,
  text: string,
  x: number,
  y: number
): void {
  ctx.font = LABEL_FONT;
  ctx.textAlign = 'left';
  ctx.textBaseline = 'alphabetic';
  ctx.lineWidth = LABEL_OUTLINE_LINE_WIDTH;
  ctx.strokeStyle = TEXT_OUTLINE_COLOR;
  ctx.lineJoin = 'round';
  ctx.strokeText(text, x, y);
  ctx.fillStyle = TEXT_FILL_COLOR;
  ctx.fillText(text, x, y);
}

export function drawTickRow(
  ctx: CanvasRenderingContext2D,
  canvasW: number,
  trackW: number,
  view: ViewWindow,
  msToX: (ms: number) => number
): void {
  // Opaque background across the whole row: drawn last (on top of scrolled
  // content, see useTimelineScene.ts) so it must cover rather than rely on
  // painting over an empty canvas.
  ctx.fillStyle = TICK_ROW_BG_COLOR;
  ctx.fillRect(0, 0, canvasW, TICK_H);

  // Tick marks + time labels, clipped to the track area so labels don't bleed
  // into either gutter
  const tickIntervalMs = chooseTickInterval(view.duration, trackW);
  const viewEnd = view.start + view.duration;
  const firstTick = Math.max(0, Math.floor(view.start / tickIntervalMs) * tickIntervalMs);
  ctx.save();
  ctx.beginPath();
  ctx.rect(LABEL_W, 0, trackW, TICK_H);
  ctx.clip();
  ctx.font = LABEL_FONT;
  ctx.textAlign = 'left';
  ctx.textBaseline = 'alphabetic';
  for (let ms = firstTick; ms <= viewEnd; ms += tickIntervalMs) {
    const tickX = msToX(ms);
    ctx.fillStyle = TICK_MARK_COLOR;
    ctx.fillRect(tickX, 0, 1, TICK_H);
    ctx.fillStyle = TICK_LABEL_COLOR;
    ctx.fillText(formatTickLabel(ms, tickIntervalMs), tickX + 3, TICK_H - 3);
  }
  ctx.restore();
}

export type DeckRowChrome = {
  zebraIndex: number;
  accent: string;
  audible: boolean;
  solo: boolean;
  muted: boolean;
};

export function drawDeckRowChrome(
  ctx: CanvasRenderingContext2D,
  row: RowLayout,
  canvasW: number,
  chrome: DeckRowChrome,
  mixerId: string
): void {
  ctx.fillStyle =
    chrome.zebraIndex % 2 === 0 ? DECK_ROW_ZEBRA_COLOR_EVEN : DECK_ROW_ZEBRA_COLOR_ODD;
  ctx.fillRect(0, row.top, canvasW, row.height);

  ctx.font = BOLD_LABEL_FONT;
  ctx.fillStyle = chrome.audible ? chrome.accent : DECK_LABEL_INACTIVE_COLOR;
  ctx.textAlign = 'center';
  ctx.textBaseline = 'middle';
  ctx.fillText(row.deckId, LABEL_W / 2, row.top + row.waveformHeight / 2);
  if (chrome.solo || chrome.muted) {
    ctx.font = BOLD_SUB_LABEL_FONT;
    ctx.fillStyle = chrome.solo ? SOLO_COLOR : MUTE_COLOR;
    ctx.fillText(
      chrome.solo ? 'S' : 'M',
      LABEL_W / 2,
      row.top + row.waveformHeight / 2 + SOLO_MUTE_LABEL_OFFSET_PX
    );
  }

  // The single automation lane's label doubles as a dropdown: its code (e.g.
  // "RT") plus a caret, drawn in the label column at the lane's vertical center.
  // Timeline.vue hit-tests this region to open the lane picker.
  if (row.lanes.length > 0) {
    const lane = row.lanes[0];
    const centerY = lane.top + lane.height / 2;
    ctx.fillStyle = LANE_DROPDOWN_COLOR;
    ctx.font = BOLD_LABEL_FONT;
    ctx.fillText(
      laneSpecs(mixerId)[lane.key].shortLabel,
      LABEL_W / 2,
      centerY - LANE_LABEL_OFFSET_PX
    );
    ctx.font = SUB_LABEL_FONT;
    ctx.fillText('▾', LABEL_W / 2, centerY + LANE_CARET_OFFSET_PX);
  }
}

export function drawMasterRowChrome(
  ctx: CanvasRenderingContext2D,
  top: number,
  height: number,
  canvasW: number,
  lane: MasterLaneKey,
  mixerId: string
): void {
  ctx.fillStyle = MASTER_ROW_BG_COLOR;
  ctx.fillRect(0, top, canvasW, height);
  ctx.font = BOLD_LABEL_FONT;
  ctx.fillStyle = MASTER_LABEL_COLOR;
  ctx.textAlign = 'center';
  ctx.textBaseline = 'middle';
  ctx.fillText('M', LABEL_W / 2, top + height / 2 - LANE_LABEL_OFFSET_PX);
  // Which of the master lanes is drawn, as a dropdown like the deck rows'.
  ctx.fillStyle = LANE_DROPDOWN_COLOR;
  ctx.font = SUB_LABEL_FONT;
  ctx.fillText(
    `${laneSpecs(mixerId)[lane].shortLabel} ▾`,
    LABEL_W / 2,
    top + height / 2 + LANE_CARET_OFFSET_PX
  );
  drawRowDivider(ctx, top + height - ROW_DIVIDER_H, canvasW);
}

export function drawValueGesturePreview(
  ctx: CanvasRenderingContext2D,
  preview: { top: number; height: number; min: number; max: number },
  points: LanePoint[],
  label: string,
  labelMs: number,
  msToX: (ms: number) => number,
  canvasW: number
): void {
  if (points.length === 0) return;
  // The line is bounded to its lane (centered strokes would otherwise spill past
  // the dividers at extreme values); the label is drawn after, outside the clip.
  withLaneClip(ctx, preview.top, preview.height, canvasW, () => {
    ctx.strokeStyle = GESTURE_PREVIEW_LINE_COLOR;
    ctx.lineWidth = GESTURE_PREVIEW_LINE_WIDTH;
    ctx.beginPath();
    let prevY = valueToY(preview.top, preview.height, preview.min, preview.max, points[0].value);
    ctx.moveTo(msToX(points[0].ms), prevY);
    for (let pointIdx = 1; pointIdx < points.length; pointIdx++) {
      const stepX = msToX(points[pointIdx].ms);
      const stepY = valueToY(
        preview.top,
        preview.height,
        preview.min,
        preview.max,
        points[pointIdx].value
      );
      ctx.lineTo(stepX, prevY);
      ctx.lineTo(stepX, stepY);
      prevY = stepY;
    }
    ctx.stroke();
  });

  const labelX = Math.min(
    msToX(labelMs) + GESTURE_LABEL_CURSOR_GAP_PX,
    canvasW - PADDING - VALUE_PREVIEW_LABEL_RIGHT_MARGIN_PX
  );
  drawOutlinedLabel(ctx, label, labelX, preview.top - VALUE_PREVIEW_LABEL_Y_OFFSET_PX);
}

export function drawNudgeGesturePreview(
  ctx: CanvasRenderingContext2D,
  startMs: number,
  endMs: number,
  percent: number,
  rowTop: number,
  rowH: number,
  cursorMs: number,
  msToX: (ms: number) => number,
  canvasW: number
): void {
  drawNudgeSpans(ctx, [{ startMs, endMs, percent }], rowTop, rowH, msToX);
  const label = `${percent > 0 ? '+' : ''}${percent}%`;
  const labelX = Math.min(
    msToX(cursorMs) + GESTURE_LABEL_CURSOR_GAP_PX,
    canvasW - PADDING - NUDGE_PREVIEW_LABEL_RIGHT_MARGIN_PX
  );
  const labelY =
    percent > 0
      ? rowTop + NUDGE_PREVIEW_LABEL_POSITIVE_Y_OFFSET_PX
      : rowTop + rowH - NUDGE_PREVIEW_LABEL_NEGATIVE_Y_OFFSET_PX;
  drawOutlinedLabel(ctx, label, labelX, labelY);
}

export function drawPaintGesturePreview(
  ctx: CanvasRenderingContext2D,
  startMs: number,
  endMs: number,
  want: boolean,
  top: number,
  height: number,
  cursorMs: number,
  msToX: (ms: number) => number,
  canvasW: number
): void {
  const paintX = msToX(startMs);
  const paintW = Math.max(1, msToX(endMs) - paintX);
  ctx.fillStyle = want ? PAINT_PREVIEW_ON_COLOR : PAINT_PREVIEW_OFF_COLOR;
  ctx.fillRect(paintX, top, paintW, height);
  const labelX = Math.min(
    msToX(cursorMs) + GESTURE_LABEL_CURSOR_GAP_PX,
    canvasW - PADDING - PAINT_PREVIEW_LABEL_RIGHT_MARGIN_PX
  );
  drawOutlinedLabel(ctx, want ? 'ON' : 'OFF', labelX, top - PAINT_PREVIEW_LABEL_Y_OFFSET_PX);
}

export function drawClipGhosts(
  ctx: CanvasRenderingContext2D,
  ghosts: { startMs: number; endMs: number }[],
  rowTop: number,
  rowH: number,
  accent: string,
  label: string,
  labelMs: number,
  msToX: (ms: number) => number,
  canvasW: number
): void {
  ctx.fillStyle = accent + CLIP_GHOST_FILL_ALPHA;
  ctx.strokeStyle = accent;
  ctx.lineWidth = 1;
  for (const ghost of ghosts) {
    const ghostX = msToX(ghost.startMs);
    const ghostW = Math.max(1, msToX(ghost.endMs) - ghostX);
    ctx.fillRect(ghostX, rowTop, ghostW, rowH);
    ctx.strokeRect(ghostX + 0.5, rowTop + 0.5, ghostW - 1, rowH - 1);
  }
  const labelX = Math.min(
    msToX(labelMs) + GESTURE_LABEL_CURSOR_GAP_PX,
    canvasW - PADDING - CLIP_GHOST_LABEL_RIGHT_MARGIN_PX
  );
  drawOutlinedLabel(ctx, label, labelX, rowTop + CLIP_GHOST_LABEL_Y_OFFSET_PX);
}

// One row divider, drawn full-width (including the label column). Used below
// each deck row and below the master row so every row separates the same way.
function drawRowDivider(ctx: CanvasRenderingContext2D, dividerY: number, canvasW: number): void {
  ctx.fillStyle = ROW_DIVIDER_GAP_COLOR;
  ctx.fillRect(0, dividerY, canvasW, ROW_DIVIDER_H);
  ctx.fillStyle = ROW_DIVIDER_LINE_COLOR;
  ctx.fillRect(0, dividerY, canvasW, ROW_DIVIDER_LINE_H);
}

export function drawRowDividers(
  ctx: CanvasRenderingContext2D,
  rows: RowLayout[],
  canvasW: number
): void {
  for (const row of rows) {
    drawRowDivider(ctx, row.top + row.height - ROW_DIVIDER_H, canvasW);
  }
}

export function drawPlayhead(ctx: CanvasRenderingContext2D, x: number, bottomY: number): void {
  ctx.strokeStyle = PLAYHEAD_COLOR;
  ctx.lineWidth = PLAYHEAD_LINE_WIDTH;
  ctx.globalAlpha = PLAYHEAD_ALPHA;
  ctx.beginPath();
  ctx.moveTo(x, 0);
  ctx.lineTo(x, bottomY);
  ctx.stroke();
  ctx.globalAlpha = 1;
}

export function drawFrameGutters(
  ctx: CanvasRenderingContext2D,
  canvasW: number,
  canvasH: number
): void {
  ctx.fillStyle = FRAME_GUTTER_COLOR;
  ctx.fillRect(LABEL_W - 1, 0, 1, canvasH);
  ctx.fillRect(canvasW - PADDING, 0, 1, canvasH);
}
