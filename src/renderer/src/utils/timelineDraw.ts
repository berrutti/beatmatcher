import type {
  Clip,
  WaveSegment,
  LoadedSpan,
  DeckLanes,
  LanePoint,
  NudgeSpan
} from '@renderer/utils/types';
import { DECK_ACCENTS, DeckId } from '@renderer/stores/decks';
import { EQ_MIN_DB, EQ_MAX_DB, FILTER_DEAD_ZONE } from '@renderer/stores/mixer';
import { formatMs } from '@renderer/utils/time';
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

export const LANE_KEYS = ['gain', 'filter', 'rate', 'eqLow', 'eqMid', 'eqHigh'] as const;
export type LaneKey = (typeof LANE_KEYS)[number];

const LANE_GROUP: Record<LaneKey, number> = {
  gain: 0,
  filter: 1,
  rate: 2,
  eqLow: 3,
  eqMid: 3,
  eqHigh: 3
};
const LANE_SHORT_LABELS: Record<LaneKey, string> = {
  gain: 'G',
  filter: 'F',
  rate: 'RT',
  eqLow: 'LO',
  eqMid: 'MD',
  eqHigh: 'HI'
};

const BEAT_LINE_COLOR = '#ffffff1f';
const DECK_LABEL_INACTIVE_COLOR = '#555';
const DOWNBEAT_LINE_COLOR = '#ffffff4d';
const EQ_BAND_COLORS_HIGH = '#3b82f6';
const EQ_BAND_COLORS_LOW = '#ef4444';
const EQ_BAND_COLORS_MID = '#eab308';
const FILTER_ACTIVE_FILL = '#ffffff10';
const FILTER_HPF_COLOR = '#fb923c';
const FILTER_LPF_COLOR = '#38bdf8';
const FILTER_NEUTRAL_COLOR = '#666666';
const GAIN_COLOR = '#e5e5e5';
const GAIN_MIN = 0;
const GAIN_MAX = 1;
// Tighter inset than the deck lanes' laneValuePad, sized for the short master row.
const MASTER_GAIN_INSET_Y = 2;
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
const RATE_COLOR = '#a78bfa';
const SOLO_COLOR = '#eab308';

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
  return Math.min(8, height / 4);
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
  if (value < -FILTER_DEAD_ZONE) return FILTER_LPF_COLOR;
  if (value > FILTER_DEAD_ZONE) return FILTER_HPF_COLOR;
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
  const endIdx = Math.min(sampleCount - 1, Math.max(startIdx, (endFrac * sampleCount - 1e-9) | 0));
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
  if (clipX1 - clipX0 < 2) return;

  const centerY = rectY + rectHeight / 2;
  const maxBarHalf = rectHeight * 0.45;
  const base = waveform.base;

  ctx.save();
  ctx.beginPath();
  ctx.rect(clipX0, rectY, clipX1 - clipX0, rectHeight);
  ctx.clip();
  ctx.fillStyle = accent + 'aa';

  // Only draw columns inside the visible track area: when zoomed in a clip can
  // be tens of thousands of pixels wide and drawing offscreen columns froze the UI.
  const visibleLeft = Math.max(LABEL_W, clipX0);
  const visibleRight = Math.min(ctx.canvas.clientWidth - PADDING, clipX1);

  for (const seg of clipWaveSegments(clip)) {
    const segX0 = msToX(seg.wallStartMs);
    const segWidth = msToX(seg.wallEndMs) - segX0;
    if (segWidth < 1) continue;
    const segTrackSpan = seg.trackEndSec - seg.trackStartSec;

    const columnStart = Math.max(0, Math.floor(Math.max(visibleLeft, segX0) - segX0));
    const columnEnd = Math.ceil(Math.min(visibleRight, segX0 + segWidth) - segX0);
    for (let column = columnStart; column < columnEnd; column++) {
      // The track-time window this column covers, then sample the high-detail
      // region first and fall back to the coarse base while detail is loading.
      const t0 = seg.trackStartSec + (column / segWidth) * segTrackSpan;
      const t1 = seg.trackStartSec + ((column + 1) / segWidth) * segTrackSpan;
      const amp = sampleRegion(waveform, t0, t1) ?? (base ? sampleRegion(base, t0, t1) : null);
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

    let step = 1;
    while (pxPerBeat * step < MIN_BEAT_SPACING_PX) step *= BEATS_PER_BAR;

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
    const labelX = Math.max(LABEL_W + 3, segX0 + 3);
    drawOutlinedLabel(ctx, bpm.toFixed(1), labelX, clipY + 10);
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
  viewEnd: number
): void {
  for (const span of deckData.filterActive) {
    if (!overlapsRange(span.startMs, span.endMs, viewStart, viewEnd)) continue;
    const spanX = msToX(span.startMs);
    const spanWidth = Math.max(1, msToX(span.endMs) - spanX);
    ctx.fillStyle = FILTER_ACTIVE_FILL;
    ctx.fillRect(spanX, laneY, spanWidth, laneH);
  }

  // Center line marks the bypass position (knob = 0); LPF sweeps below it, HPF above.
  drawLaneCenterLine(ctx, canvasWidth, laneY, laneH, -1, 1, 0);

  drawLaneSteps(
    ctx,
    deckData.filter,
    laneY,
    laneH,
    -1,
    1,
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
  ctx.strokeStyle = '#4a4a4a';
  ctx.setLineDash([4, 4]);
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

function drawEqLane(
  ctx: CanvasRenderingContext2D,
  points: LanePoint[],
  color: string,
  laneY: number,
  laneH: number,
  msToX: (ms: number) => number,
  viewStart: number,
  viewEnd: number
): void {
  drawLaneSteps(ctx, points, laneY, laneH, EQ_MIN_DB, EQ_MAX_DB, color, msToX, viewStart, viewEnd);
}

type LaneDrawer = (
  ctx: CanvasRenderingContext2D,
  canvasWidth: number,
  deckData: DeckLanes,
  laneY: number,
  laneH: number,
  msToX: (ms: number) => number,
  viewStart: number,
  viewEnd: number
) => void;

const LANE_DRAWERS: Record<LaneKey, LaneDrawer> = {
  gain: (ctx, _w, deckData, laneY, laneH, msToX, viewStart, viewEnd) =>
    drawLaneSteps(ctx, deckData.gain, laneY, laneH, 0, 1, GAIN_COLOR, msToX, viewStart, viewEnd),
  filter: drawFilterLane,
  rate: drawRateLane,
  eqLow: (ctx, _w, deckData, laneY, laneH, msToX, viewStart, viewEnd) =>
    drawEqLane(ctx, deckData.eqLow, EQ_BAND_COLORS_LOW, laneY, laneH, msToX, viewStart, viewEnd),
  eqMid: (ctx, _w, deckData, laneY, laneH, msToX, viewStart, viewEnd) =>
    drawEqLane(ctx, deckData.eqMid, EQ_BAND_COLORS_MID, laneY, laneH, msToX, viewStart, viewEnd),
  eqHigh: (ctx, _w, deckData, laneY, laneH, msToX, viewStart, viewEnd) =>
    drawEqLane(ctx, deckData.eqHigh, EQ_BAND_COLORS_HIGH, laneY, laneH, msToX, viewStart, viewEnd)
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
  viewEnd: number
): void {
  for (let laneIdx = 0; laneIdx < sublanes.length; laneIdx++) {
    const { key, top, height } = sublanes[laneIdx];
    const group = LANE_GROUP[key];
    const prevGroup = laneIdx > 0 ? LANE_GROUP[sublanes[laneIdx - 1].key] : -1;
    const trackW = canvasWidth - LABEL_W - PADDING;

    ctx.fillStyle = group % 2 === 0 ? '#1a1a1a' : '#141414';
    ctx.fillRect(LABEL_W, top, trackW, height);

    // Frame the lane with a top border so it reads as a bounded panel rather
    // than bleeding into the waveform above. Drawn regardless of data so the
    // separation is consistent whether or not the deck/lane has content.
    ctx.fillStyle = laneIdx > 0 && group !== prevGroup ? '#2a2a2a' : '#2e2e2e';
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
        viewEnd
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

  ctx.fillStyle = accent + '18';
  ctx.fillRect(spanX, spanY, spanWidth, spanHeight);

  ctx.strokeStyle = accent + '40';
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
  if (spanWidth <= 40) return;
  const spanY = rowY + CLIP_BAND_INSET_Y;
  const spanHeight = rowH - 2 * CLIP_BAND_INSET_Y;
  ctx.save();
  ctx.beginPath();
  ctx.rect(spanX + 3, spanY, spanWidth - 6, spanHeight);
  ctx.clip();
  ctx.font = '9px monospace';
  ctx.textAlign = 'left';
  ctx.textBaseline = 'middle';
  const textX = Math.max(LABEL_W + 3, spanX + 3);
  const textY = spanY + spanHeight / 2;
  ctx.lineWidth = 3;
  ctx.strokeStyle = '#000000cc';
  ctx.lineJoin = 'round';
  ctx.strokeText(span.trackName, textX, textY);
  ctx.fillStyle = '#ffffff';
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

  ctx.fillStyle = accent + '55';
  ctx.fillRect(clipX, clipY, clipWidth, clipHeight);

  drawClipWaveform(ctx, clip, waveform, clipY, clipHeight, accent, msToX);
  drawClipBeatGrid(ctx, clip, clipY, clipHeight, msToX);

  ctx.strokeStyle = accent + 'cc';
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
  ctx.fillStyle = 'rgba(255, 255, 255, 0.14)';
  ctx.fillRect(selectionX, selectionY, selectionWidth, selectionHeight);
  ctx.strokeStyle = 'rgba(255, 255, 255, 0.9)';
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

  ctx.fillStyle = '#0a0a0a';
  ctx.fillRect(0, stripY, canvasWidth, OVERVIEW_H);

  const laneHeight = (OVERVIEW_H - 4) / DECK_ORDER.length;
  for (let deckIdx = 0; deckIdx < DECK_ORDER.length; deckIdx++) {
    const deckId = DECK_ORDER[deckIdx];
    const accent = accents[deckId] ?? DECK_ACCENTS[deckId];
    const overviewLaneY = stripY + 2 + deckIdx * laneHeight;
    ctx.fillStyle = accent + 'aa';
    for (const clip of clips) {
      if (clip.deck !== deckId) continue;
      const clipX = fullMsToX(clip.sessionStartMs);
      const clipWidth = Math.max(1, fullMsToX(clip.sessionEndMs) - clipX);
      ctx.fillRect(clipX, overviewLaneY, clipWidth, Math.max(1, laneHeight - 1));
    }
  }

  if (playheadMs > 0) {
    ctx.fillStyle = '#ffffffcc';
    ctx.fillRect(fullMsToX(playheadMs) - 0.5, stripY, 1, OVERVIEW_H);
  }

  const viewportX = fullMsToX(viewStart);
  const viewportWidth = Math.max(2, fullMsToX(viewEnd) - viewportX);
  ctx.fillStyle = '#ffffff15';
  ctx.fillRect(viewportX, stripY, viewportWidth, OVERVIEW_H);
  ctx.strokeStyle = '#ffffff80';
  ctx.lineWidth = 1;
  ctx.strokeRect(viewportX + 0.5, stripY + 0.5, Math.max(1, viewportWidth - 1), OVERVIEW_H - 1);

  ctx.fillStyle = '#222';
  ctx.fillRect(0, stripY - 1, canvasWidth, 1);

  return { y: stripY, h: OVERVIEW_H };
}

export function drawMasterGainLane(
  ctx: CanvasRenderingContext2D,
  points: LanePoint[],
  masterTopY: number,
  masterRowH: number,
  canvasWidth: number,
  msToX: (ms: number) => number,
  viewStart: number,
  viewEnd: number
): void {
  // Clip to the track area like the deck lanes do, so the level line never
  // bleeds left into the "M" label gutter or right into the padding.
  withLaneClip(ctx, masterTopY, masterRowH, canvasWidth, () =>
    drawLaneSteps(
      ctx,
      points,
      masterTopY + MASTER_GAIN_INSET_Y,
      masterRowH - 2 * MASTER_GAIN_INSET_Y,
      GAIN_MIN,
      GAIN_MAX,
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

// ── draw() orchestration pieces ──────────────────────────────────────────────
// Renderers extracted from Timeline.vue's draw(): each takes the context plus
// explicit data, no component state, so draw() stays a short orchestrator.

function drawOutlinedLabel(
  ctx: CanvasRenderingContext2D,
  text: string,
  x: number,
  y: number
): void {
  ctx.font = '9px monospace';
  ctx.textAlign = 'left';
  ctx.textBaseline = 'alphabetic';
  ctx.lineWidth = 3;
  ctx.strokeStyle = '#000000cc';
  ctx.lineJoin = 'round';
  ctx.strokeText(text, x, y);
  ctx.fillStyle = '#ffffff';
  ctx.fillText(text, x, y);
}

export function drawTickRow(
  ctx: CanvasRenderingContext2D,
  canvasW: number,
  trackW: number,
  view: ViewWindow,
  msToX: (ms: number) => number
): void {
  // Fill tick-row gutters so they match the surrounding row background color
  ctx.fillStyle = '#161616';
  ctx.fillRect(0, 0, LABEL_W, TICK_H);
  ctx.fillRect(canvasW - PADDING, 0, PADDING, TICK_H);

  // Tick marks + time labels, clipped to the track area so labels don't bleed
  // into either gutter
  const tickIntervalMs = chooseTickInterval(view.duration, trackW);
  const viewEnd = view.start + view.duration;
  const firstTick = Math.max(0, Math.floor(view.start / tickIntervalMs) * tickIntervalMs);
  ctx.save();
  ctx.beginPath();
  ctx.rect(LABEL_W, 0, trackW, TICK_H);
  ctx.clip();
  ctx.font = `9px monospace`;
  ctx.textAlign = 'left';
  ctx.textBaseline = 'alphabetic';
  for (let ms = firstTick; ms <= viewEnd; ms += tickIntervalMs) {
    const tickX = msToX(ms);
    ctx.fillStyle = '#333';
    ctx.fillRect(tickX, 0, 1, TICK_H);
    ctx.fillStyle = '#555';
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
  chrome: DeckRowChrome
): void {
  ctx.fillStyle = chrome.zebraIndex % 2 === 0 ? '#161616' : '#131313';
  ctx.fillRect(0, row.top, canvasW, row.height);

  ctx.font = `bold 9px monospace`;
  ctx.fillStyle = chrome.audible ? chrome.accent : DECK_LABEL_INACTIVE_COLOR;
  ctx.textAlign = 'center';
  ctx.textBaseline = 'middle';
  ctx.fillText(row.deckId, LABEL_W / 2, row.top + row.waveformHeight / 2);
  if (chrome.solo || chrome.muted) {
    ctx.font = `bold 7px monospace`;
    ctx.fillStyle = chrome.solo ? SOLO_COLOR : MUTE_COLOR;
    ctx.fillText(chrome.solo ? 'S' : 'M', LABEL_W / 2, row.top + row.waveformHeight / 2 + 11);
  }

  // The single automation lane's label doubles as a dropdown: its code (e.g.
  // "RT") plus a caret, drawn in the label column at the lane's vertical center.
  // Timeline.vue hit-tests this region to open the lane picker.
  if (row.lanes.length > 0) {
    const lane = row.lanes[0];
    const cy = lane.top + lane.height / 2;
    ctx.fillStyle = LANE_DROPDOWN_COLOR;
    ctx.font = `bold 9px monospace`;
    ctx.fillText(LANE_SHORT_LABELS[lane.key], LABEL_W / 2, cy - 5);
    ctx.font = `7px monospace`;
    ctx.fillText('▾', LABEL_W / 2, cy + 6);
  }
}

export function drawMasterRowChrome(
  ctx: CanvasRenderingContext2D,
  top: number,
  height: number,
  canvasW: number
): void {
  ctx.fillStyle = '#101010';
  ctx.fillRect(0, top, canvasW, height);
  ctx.font = `bold 9px monospace`;
  ctx.fillStyle = MASTER_LABEL_COLOR;
  ctx.textAlign = 'center';
  ctx.textBaseline = 'middle';
  ctx.fillText('M', LABEL_W / 2, top + height / 2);
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
    ctx.strokeStyle = '#ffffffcc';
    ctx.lineWidth = 1.5;
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

  const labelX = Math.min(msToX(labelMs) + 8, canvasW - PADDING - 40);
  drawOutlinedLabel(ctx, label, labelX, preview.top - 6);
}

export function drawNudgeGesturePreview(
  ctx: CanvasRenderingContext2D,
  t0: number,
  t1: number,
  percent: number,
  rowTop: number,
  rowH: number,
  cursorMs: number,
  msToX: (ms: number) => number,
  canvasW: number
): void {
  drawNudgeSpans(ctx, [{ startMs: t0, endMs: t1, percent }], rowTop, rowH, msToX);
  const label = `${percent > 0 ? '+' : ''}${percent}%`;
  const labelX = Math.min(msToX(cursorMs) + 8, canvasW - PADDING - 30);
  const labelY = percent > 0 ? rowTop + 12 : rowTop + rowH - 6;
  drawOutlinedLabel(ctx, label, labelX, labelY);
}

export function drawPaintGesturePreview(
  ctx: CanvasRenderingContext2D,
  t0: number,
  t1: number,
  want: boolean,
  top: number,
  height: number,
  cursorMs: number,
  msToX: (ms: number) => number,
  canvasW: number
): void {
  const x0 = msToX(t0);
  const paintW = Math.max(1, msToX(t1) - x0);
  ctx.fillStyle = want ? '#ffffff30' : '#00000060';
  ctx.fillRect(x0, top, paintW, height);
  const labelX = Math.min(msToX(cursorMs) + 8, canvasW - PADDING - 30);
  drawOutlinedLabel(ctx, want ? 'ON' : 'OFF', labelX, top - 6);
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
  ctx.fillStyle = accent + '50';
  ctx.strokeStyle = accent;
  ctx.lineWidth = 1;
  for (const ghost of ghosts) {
    const ghostX = msToX(ghost.startMs);
    const ghostW = Math.max(1, msToX(ghost.endMs) - ghostX);
    ctx.fillRect(ghostX, rowTop, ghostW, rowH);
    ctx.strokeRect(ghostX + 0.5, rowTop + 0.5, ghostW - 1, rowH - 1);
  }
  const labelX = Math.min(msToX(labelMs) + 8, canvasW - PADDING - 60);
  drawOutlinedLabel(ctx, label, labelX, rowTop + 12);
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
  ctx.strokeStyle = '#ffffff';
  ctx.lineWidth = 1.5;
  ctx.globalAlpha = 0.9;
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
  ctx.fillStyle = '#2a2a2a';
  ctx.fillRect(LABEL_W - 1, 0, 1, canvasH);
  ctx.fillRect(canvasW - PADDING, 0, 1, canvasH);
}
