import type {
  Clip,
  WaveSegment,
  LoadedSpan,
  DeckLanes,
  LanePoint,
  EditableLaneKey,
  MasterLaneKey,
  DeckLaneKey,
  DeckId
} from '@renderer/utils/types';
import { DECK_ACCENTS, DEFAULT_METER } from '@renderer/utils/types';
import { editConstants, laneSpecs, type LaneSpec } from '@renderer/utils/sessionCore';
import { jogLaneColumns } from '@renderer/utils/jogLane';
import { formatMs } from '@renderer/utils/time';
import { beatLineStep } from '@renderer/utils/beatGrid';
import {
  overlapsRange,
  msToFrac,
  sliceVisiblePoints,
  chooseTickInterval,
  type ViewWindow
} from '@renderer/utils/timelineView';

// A region rather than the whole track: zoom refetches a tighter range at
// higher point density.
export type WaveformRegion = { startSec: number; endSec: number; amps: Float32Array };
// `base` is a coarse slice of the whole extent, so panning to an unfetched spot
// still shows something until the detail arrives.
export type TrackWaveform = WaveformRegion & { base?: WaveformRegion };

export const DECK_ORDER = ['A', 'B', 'C', 'D'] as const;
// Tall enough for the waveform and beat grid to stay legible together.
export const ROW_H = 80;
export const LABEL_W = 32;
export const TICK_H = 16;
export const PADDING = 12;
export const OVERVIEW_H = 22;
export const OVERVIEW_GAP = 4;

// The wheel lane has no manifest spec: it plots gestures, not a mixer param.
type SpecLaneKey = Exclude<DeckLaneKey, 'jog'>;

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
const DECK_ROW_BG_COLOR = '#141414';
const DECK_ROW_TINT_ALPHA = '14';
const LANE_TINT_ALPHA = '10';
const DOWNBEAT_LINE_COLOR = '#ffffff4d';
const EQ_BAND_COLORS_HIGH = '#3b82f6';
const EQ_BAND_COLORS_LOW = '#ef4444';
const EQ_BAND_COLORS_MID = '#eab308';
const FILTER_ACTIVE_FILL = '#ffffff10';

const FILTER_COLOR = '#38bdf8';
const FRAME_GUTTER_COLOR = '#2a2a2a';
const GAIN_COLOR = '#e5e5e5';
const GESTURE_LABEL_CURSOR_GAP_PX = 8;
const GESTURE_PREVIEW_LINE_COLOR = '#ffffffcc';
const GESTURE_PREVIEW_LINE_WIDTH = 1.5;
const LABEL_FONT = '9px monospace';
const BOLD_LABEL_FONT = 'bold 9px monospace';
const BOLD_SUB_LABEL_FONT = 'bold 7px monospace';
const LABEL_OUTLINE_LINE_WIDTH = 3;
const LANE_BORDER_COLOR = '#2a2a2a';
// The deck word steps aside only when a solo/mute badge shares its column.
const LANE_LABEL_ROTATED_DX = 5;
const LANE_WAVEFORM_ALPHA = 0.18;
const LANE_WAVEFORM_COLOR = '#ffffff';
const LANE_CENTER_LINE_COLOR = '#4a4a4a';
const LANE_DEFAULT_ALPHA = 0.3;
const LANE_LINE_WIDTH = 1.5;
const LANE_STEP_CORNER_RADIUS = 4;
const LANE_DEFAULT_DIM_MIN_PX = 64;
const LANE_SLIVER_MIN_PX = 8;
const LANE_HIGHLIGHT_COLOR = '#ffffff';
const LANE_DEFAULT_TOLERANCE = 0.001;
const LANE_BG_COLOR = '#1a1a1a';
const LANE_VALUE_PAD_FRACTION = 4;
const LANE_VALUE_PAD_MAX_PX = 8;
const LOADED_SPAN_FILL_ALPHA = '18';
const LOADED_SPAN_STROKE_ALPHA = '40';
const MIN_DRAWABLE_CLIP_WIDTH_PX = 2;
const MIN_DRAWABLE_SEG_WIDTH_PX = 1;
// Tighter inset than the deck lanes' laneValuePad, sized for their far shorter
// default height.
export const MASTER_GAIN_INSET_Y = 2;
const MASTER_ROW_BG_COLOR = '#101010';
const LANE_DROPDOWN_COLOR = '#06b6d4';
export const LANE_CARET_CLOSED = '▾';
export const LANE_CARET_OPEN = '▴';
const MIN_BEAT_SPACING_PX = 8;
export const BEAT_LINE_W = 2;
const CLIP_BAND_INSET_Y = 4;
// A region narrower than this can't fit a "138.0" BPM label legibly, so it's
// skipped until the user zooms in enough to widen it.
const BPM_LABEL_MIN_PX = 30;
const DISABLED_COLOR = '#ef4444';
const JOG_FILL_COLOR = '#fbbf24cc';
const JOG_SCALE_LABEL_COLOR = '#777';
const JOG_LANE_RULE_H = 1;
const JOG_SCALE_LABEL_INSET_PX = 3;
// Below this a tenth of a percent still reads. Above it the decimal is noise.
const JOG_SCALE_LABEL_COARSE_PCT = 10;
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
const SOLO_MUTE_LABEL_OFFSET_PX = 7;
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

export type SublaneLayout = { key: DeckLaneKey; top: number; height: number };
export type RowLayout = {
  deckId: DeckId;
  top: number;
  height: number;
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

    // Whole pixels: a bar at a fractional x spreads over two columns, and where
    // two segments meet that phase jumps and draws a lighter stripe.
    const from = Math.max(Math.round(segX0), Math.ceil(visibleLeft));
    const to = Math.min(Math.round(segX0 + segWidth), Math.floor(visibleRight));
    for (let column = from; column < to; column++) {
      const columnStartSec = seg.trackStartSec + ((column - segX0) / segWidth) * segTrackSpan;
      const columnEndSec = seg.trackStartSec + ((column + 1 - segX0) / segWidth) * segTrackSpan;
      const amp =
        sampleRegion(waveform, columnStartSec, columnEndSec) ??
        (base ? sampleRegion(base, columnStartSec, columnEndSec) : null);
      if (amp === null) continue;
      const barHeight = Math.max(1, Math.sqrt(amp) * maxBarHalf);
      ctx.fillRect(column, centerY - barHeight, 1, barHeight * 2);
    }
  }

  ctx.restore();
}

// Drawn per wave segment, mapped through each segment's track->wall rate, so the
// lines compress and stretch with the waveform when the rate changes.
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

    const step = beatLineStep(pxPerBeat, MIN_BEAT_SPACING_PX, DEFAULT_METER.beatsPerBar);

    const firstBeat = Math.ceil((seg.trackStartSec - beatOffset) / beatDurSec);
    const lastBeat = Math.floor((seg.trackEndSec - beatOffset) / beatDurSec);
    for (let beat = firstBeat; beat <= lastBeat; beat++) {
      if (beat % step !== 0) continue;
      const beatSec = beatOffset + beat * beatDurSec;
      const beatX = segX0 + ((beatSec - seg.trackStartSec) / effRate) * pxPerWallSec;
      ctx.fillStyle =
        beat % (step * DEFAULT_METER.beatsPerBar) === 0 ? DOWNBEAT_LINE_COLOR : BEAT_LINE_COLOR;
      // Rounded, or the line spreads over two columns at half coverage and reads
      // as a blur rather than a grid.
      ctx.fillRect(Math.round(beatX), rectY, BEAT_LINE_W, rectHeight);
    }
  }

  ctx.restore();
}

// Scaled by each segment's effective rate, so a region the user pitched reads
// the tempo it actually plays at.
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

type LaneView = { msToX: (ms: number) => number; viewStart: number; viewEnd: number };

type LaneBand = {
  top: number;
  height: number;
  min: number;
  max: number;
  defaultValue: number;
  // How far from the default still counts as doing nothing: the filter's dead
  // zone, where the knob is past centre but the audio is untouched.
  defaultBand: number;
};

// Held flat until the next point rather than interpolated, because an event
// fires at the moment a value changes.
function drawLaneSteps(
  ctx: CanvasRenderingContext2D,
  points: LanePoint[],
  band: LaneBand,
  color: string,
  view: LaneView,
  // Drawn in the highlight colour, so a menu can show what it would clear.
  highlight: LaneHighlight = null
): void {
  const { top: laneY, height: laneHeight, min: minVal, max: maxVal, defaultValue } = band;
  const { msToX, viewStart, viewEnd } = view;
  const defaultBand = band.defaultBand;
  const visible = sliceVisiblePoints(points, viewStart, viewEnd);
  if (visible.length < 2) return;
  const atDefault = (value: number) =>
    Math.abs(value - defaultValue) <=
    Math.max(defaultBand, (maxVal - minVal) * LANE_DEFAULT_TOLERANCE);
  ctx.lineWidth = LANE_LINE_WIDTH;
  ctx.lineJoin = 'round';
  ctx.lineCap = 'round';

  const pieces: LanePiece[] = [];
  function line(color: string, alpha: number, x0: number, y0: number, x1: number, y1: number) {
    pieces.push({ color, alpha, x0, y0, x1, y1 });
  }

  const runEndIdx = defaultRunEnds(visible, atDefault);
  let runStartX = msToX(visible[0].ms);

  for (let pointIdx = 0; pointIdx < visible.length - 1; pointIdx++) {
    const cur = visible[pointIdx];
    const next = visible[pointIdx + 1];
    const segX0 = msToX(cur.ms);
    const segX1 = msToX(next.ms);
    const stepY = valueToY(laneY, laneHeight, minVal, maxVal, cur.value);
    const nextStepY = valueToY(laneY, laneHeight, minVal, maxVal, next.value);
    const parked = atDefault(cur.value);
    if (!parked || pointIdx === 0 || !atDefault(visible[pointIdx - 1].value)) runStartX = segX0;
    // The whole run decides, not each step in it: a curve that only passes
    // through the default stays at full strength.
    const runEndX = msToX(visible[runEndIdx[pointIdx]].ms);
    const dimmed = parked && runEndX - runStartX >= LANE_DEFAULT_DIM_MIN_PX;

    // Inclusive at both ends, and never a value sitting at the default: a move
    // and the event that ends it can share a millisecond.
    const inSpan =
      highlight !== null &&
      cur.ms >= highlight.startMs &&
      cur.ms <= highlight.endMs &&
      Math.abs(cur.value - defaultValue) > (maxVal - minVal) * LANE_DEFAULT_TOLERANCE;
    const lit = inSpan ? LANE_HIGHLIGHT_COLOR : color;

    line(lit, dimmed ? LANE_DEFAULT_ALPHA : 1, segX0, stepY, segX1, stepY);
    if (nextStepY !== stepY) line(lit, 1, segX1, stepY, segX1, nextStepY);
  }
  strokePieces(ctx, absorbSlivers(pieces));
  ctx.globalAlpha = 1;
}

type LanePiece = {
  color: string;
  alpha: number;
  x0: number;
  y0: number;
  x1: number;
  y1: number;
};

// A value hovering on the dead-zone edge crosses it a pixel at a time, painting
// dashes. Only an island inside one run is absorbed; a real crossing keeps its colour.
function absorbSlivers(pieces: LanePiece[]): LanePiece[] {
  const out = pieces.map((piece) => ({ ...piece }));
  const groups: { start: number; end: number; width: number }[] = [];
  for (let idx = 0; idx < out.length; idx++) {
    const group = groups[groups.length - 1];
    const continues =
      group !== undefined &&
      out[idx].color === out[group.start].color &&
      out[idx].alpha === out[group.start].alpha;
    if (continues) {
      group.end = idx;
      group.width += Math.abs(out[idx].x1 - out[idx].x0);
    } else {
      groups.push({ start: idx, end: idx, width: Math.abs(out[idx].x1 - out[idx].x0) });
    }
  }

  for (let idx = 1; idx < groups.length - 1; idx++) {
    const group = groups[idx];
    if (group.width >= LANE_SLIVER_MIN_PX) continue;
    const before = out[groups[idx - 1].start];
    const after = out[groups[idx + 1].start];
    if (before.color !== after.color || before.alpha !== after.alpha) continue;
    for (let at = group.start; at <= group.end; at++) {
      out[at].color = before.color;
      out[at].alpha = before.alpha;
    }
  }
  return out;
}

function strokePieces(ctx: CanvasRenderingContext2D, pieces: LanePiece[]): void {
  let path: [number, number][] = [];
  let penColor = '';
  let penAlpha = 1;

  function flush(): void {
    if (path.length < 2) {
      path = [];
      return;
    }
    ctx.globalAlpha = penAlpha;
    ctx.strokeStyle = penColor;
    strokeRoundedPath(ctx, path);
    path = [];
  }

  for (const piece of pieces) {
    const last = path[path.length - 1];
    const joins =
      penColor === piece.color &&
      penAlpha === piece.alpha &&
      last?.[0] === piece.x0 &&
      last[1] === piece.y0;
    if (!joins) {
      flush();
      penColor = piece.color;
      penAlpha = piece.alpha;
      path = [[piece.x0, piece.y0]];
    }
    path.push([piece.x1, piece.y1]);
  }
  flush();
}

// For each point sitting at the default, the index the run it belongs to ends
// at: the first point off the default after it, or the last point.
function defaultRunEnds(points: LanePoint[], atDefault: (value: number) => boolean): number[] {
  const ends: number[] = new Array(points.length).fill(points.length - 1);
  for (let idx = points.length - 2; idx >= 0; idx--) {
    if (!atDefault(points[idx].value)) ends[idx] = idx;
    else ends[idx] = atDefault(points[idx + 1].value) ? ends[idx + 1] : idx + 1;
  }
  return ends;
}

function strokeRoundedPath(ctx: CanvasRenderingContext2D, points: [number, number][]): void {
  ctx.beginPath();
  ctx.moveTo(points[0][0], points[0][1]);
  for (let idx = 1; idx < points.length - 1; idx++) {
    const [prevX, prevY] = points[idx - 1];
    const [cornerX, cornerY] = points[idx];
    const [nextX, nextY] = points[idx + 1];
    const radius = Math.min(
      LANE_STEP_CORNER_RADIUS,
      Math.hypot(cornerX - prevX, cornerY - prevY) / 2,
      Math.hypot(nextX - cornerX, nextY - cornerY) / 2
    );
    ctx.arcTo(cornerX, cornerY, nextX, nextY, radius);
  }
  const [endX, endY] = points[points.length - 1];
  ctx.lineTo(endX, endY);
  ctx.stroke();
}

const drawFilterLane: LaneDrawer = (ctx, canvasWidth, deckData, lane, view, specs, highlight) => {
  const { msToX, viewStart, viewEnd } = view;
  for (const span of deckData.filterActive) {
    if (!overlapsRange(span.startMs, span.endMs, viewStart, viewEnd)) continue;
    const spanX = msToX(span.startMs);
    const spanWidth = Math.max(1, msToX(span.endMs) - spanX);
    ctx.fillStyle = FILTER_ACTIVE_FILL;
    ctx.fillRect(spanX, lane.top, spanWidth, lane.height);
  }

  const { min, max, defaultValue } = specs.filter;

  // Center line marks the bypass position (knob = 0). LPF sweeps below it, HPF above.
  drawLaneCenterLine(ctx, canvasWidth, lane.top, lane.height, min, max, defaultValue);

  drawLaneSteps(
    ctx,
    deckData.filter,
    { ...lane, min, max, defaultValue, defaultBand: editConstants().filterDeadZone },
    FILTER_COLOR,
    view,
    highlight
  );
};

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
  ctx.beginPath();
  ctx.moveTo(LABEL_W, centerY + 0.5);
  ctx.lineTo(canvasWidth - PADDING, centerY + 0.5);
  ctx.stroke();
  ctx.restore();
}

const drawRateLane: LaneDrawer = (ctx, canvasWidth, deckData, lane, view, _specs, highlight) => {
  const neutral = 1;
  // Center line marks the neutral rate (1.0 = 0% pitch).
  drawLaneCenterLine(
    ctx,
    canvasWidth,
    lane.top,
    lane.height,
    deckData.rateMin,
    deckData.rateMax,
    neutral
  );

  drawLaneSteps(
    ctx,
    deckData.rate,
    {
      ...lane,
      min: deckData.rateMin,
      max: deckData.rateMax,
      defaultValue: neutral,
      defaultBand: 0
    },
    RATE_COLOR,
    view,
    highlight
  );
};

type LaneSpecs = Record<EditableLaneKey, LaneSpec>;

// The span a menu is offering to clear, drawn in the highlight colour.
type LaneHighlight = { startMs: number; endMs: number } | null;

type LaneRect = { top: number; height: number };

type LaneDrawer = (
  ctx: CanvasRenderingContext2D,
  canvasWidth: number,
  deckData: DeckLanes,
  lane: LaneRect,
  view: LaneView,
  specs: LaneSpecs,
  highlight: LaneHighlight
) => void;

const SPEC_LANE_COLORS: Record<Exclude<SpecLaneKey, 'filter' | 'rate'>, string> = {
  gain: GAIN_COLOR,
  eqLow: EQ_BAND_COLORS_LOW,
  eqMid: EQ_BAND_COLORS_MID,
  eqHigh: EQ_BAND_COLORS_HIGH
};

function specLaneDrawer(key: keyof typeof SPEC_LANE_COLORS): LaneDrawer {
  return (ctx, _canvasWidth, deckData, lane, view, specs, highlight) => {
    const { min, max, defaultValue } = specs[key];
    drawLaneSteps(
      ctx,
      deckData[key],
      { ...lane, min, max, defaultValue, defaultBand: 0 },
      SPEC_LANE_COLORS[key],
      view,
      highlight
    );
  };
}

const LANE_DRAWERS: Record<SpecLaneKey, LaneDrawer> = {
  gain: specLaneDrawer('gain'),
  filter: drawFilterLane,
  rate: drawRateLane,
  eqLow: specLaneDrawer('eqLow'),
  eqMid: specLaneDrawer('eqMid'),
  eqHigh: specLaneDrawer('eqHigh')
};

// One place, so no drawer has to guard its own edges against the lane dividers.
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

// The same waveform the clip band shows, dimmed under each automation lane, so a
// curve is read against the audio it shapes rather than against an empty strip.
export function drawLaneWaveform(
  ctx: CanvasRenderingContext2D,
  clips: Clip[],
  waveforms: Map<string, TrackWaveform>,
  msToX: (ms: number) => number,
  top: number,
  height: number
): void {
  const bandHalf = height * BAR_HALF_HEIGHT_FRACTION;
  const bandTop = top + height / 2 - bandHalf;
  ctx.save();
  ctx.globalAlpha = LANE_WAVEFORM_ALPHA;
  for (const clip of clips) {
    drawClipWaveform(
      ctx,
      clip,
      waveforms.get(clip.trackPath),
      top,
      height,
      LANE_WAVEFORM_COLOR,
      msToX
    );
    // Held to the band the bars fill: a full-height rule in every lane would
    // read as a ruler competing with the curve drawn over it.
    drawClipBeatGrid(ctx, clip, bandTop, bandHalf * 2, msToX);
  }
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
  mixerId: string,
  clips: Clip[],
  waveforms: Map<string, TrackWaveform>,
  accent: string,
  highlight: { lane: EditableLaneKey; startMs: number; endMs: number } | null = null
): void {
  const specs = laneSpecs(mixerId);
  for (let laneIdx = 0; laneIdx < sublanes.length; laneIdx++) {
    const { key, top, height } = sublanes[laneIdx];
    if (key === 'jog') continue;
    const trackW = canvasWidth - LABEL_W - PADDING;

    ctx.fillStyle = LANE_BG_COLOR;
    ctx.fillRect(LABEL_W, top, trackW, height);
    ctx.fillStyle = accent + LANE_TINT_ALPHA;
    ctx.fillRect(LABEL_W, top, trackW, height);
    drawLaneWaveform(ctx, clips, waveforms, msToX, top, height);

    // Drawn whether or not the lane has data, so an empty one is still bounded.
    ctx.fillStyle = LANE_BORDER_COLOR;
    ctx.fillRect(LABEL_W, top, trackW, 1);

    // The value curve needs deck data. The frame above does not.
    if (!deckData) continue;

    // Inset the value area so the curve breathes and never touches the frame;
    // the center stays at the lane's exact middle, keeping the halves symmetric.
    const pad = laneValuePad(height);
    withLaneClip(ctx, top, height, canvasWidth, () =>
      LANE_DRAWERS[key](
        ctx,
        canvasWidth,
        deckData,
        { top: top + pad, height: height - 2 * pad },
        { msToX, viewStart, viewEnd },
        specs,
        highlight?.lane === key ? { startMs: highlight.startMs, endMs: highlight.endMs } : null
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

// Zero is centred: above the line is forward, below is reverse, height is speed.
export function drawJogLane(
  ctx: CanvasRenderingContext2D,
  canvasWidth: number,
  laneY: number,
  laneH: number,
  curve: LanePoint[],
  xToMs: (x: number) => number,
  scale: number,
  clips: Clip[],
  waveforms: Map<string, TrackWaveform>,
  msToX: (ms: number) => number,
  accent: string
): void {
  const trackW = canvasWidth - LABEL_W - PADDING;
  ctx.fillStyle = LANE_BG_COLOR;
  ctx.fillRect(LABEL_W, laneY, trackW, laneH);
  ctx.fillStyle = accent + LANE_TINT_ALPHA;
  ctx.fillRect(LABEL_W, laneY, trackW, laneH);
  drawLaneWaveform(ctx, clips, waveforms, msToX, laneY, laneH);
  ctx.fillStyle = LANE_BORDER_COLOR;
  ctx.fillRect(LABEL_W, laneY, trackW, JOG_LANE_RULE_H);

  const centerY = laneY + laneH / 2;
  const halfH = laneH / 2 - laneValuePad(laneH);
  ctx.fillStyle = LANE_CENTER_LINE_COLOR;
  ctx.fillRect(LABEL_W, centerY, trackW, JOG_LANE_RULE_H);
  if (halfH <= 0) return;

  const columns = jogLaneColumns(curve, Math.ceil(trackW), (column) => xToMs(LABEL_W + column));

  ctx.fillStyle = JOG_FILL_COLOR;
  for (let column = 0; column < columns.length; column++) {
    // Clamped rather than auto-scaled: the height is the range a gesture can
    // author, so a recorded spike clips instead of shrinking everything else.
    const height = clampToLane((columns[column] / scale) * halfH, halfH);
    if (height === 0) continue;
    ctx.fillRect(LABEL_W + column, centerY - Math.max(height, 0), 1, Math.abs(height));
  }

  drawJogLaneScale(ctx, scale, laneY);
}

function clampToLane(height: number, halfH: number): number {
  return Math.max(-halfH, Math.min(halfH, height));
}

function drawJogLaneScale(ctx: CanvasRenderingContext2D, scale: number, laneY: number): void {
  ctx.fillStyle = JOG_SCALE_LABEL_COLOR;
  ctx.font = LABEL_FONT;
  ctx.textAlign = 'left';
  ctx.textBaseline = 'top';
  const digits = scale < JOG_SCALE_LABEL_COARSE_PCT ? 1 : 0;
  ctx.fillText(
    `±${scale.toFixed(digits)}%`,
    LABEL_W + JOG_SCALE_LABEL_INSET_PX,
    laneY + JOG_SCALE_LABEL_INSET_PX
  );
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
  mixerId: string,
  highlight: LaneHighlight = null
): void {
  // Clip to the track area like the deck lanes do, so the level line never
  // bleeds left into the "M" label gutter or right into the padding.
  const { min, max, defaultValue } = laneSpecs(mixerId)[lane];
  withLaneClip(ctx, masterTopY, masterRowH, canvasWidth, () =>
    drawLaneSteps(
      ctx,
      points,
      {
        top: masterTopY + MASTER_GAIN_INSET_Y,
        height: masterRowH - 2 * MASTER_GAIN_INSET_Y,
        min,
        max,
        defaultValue,
        defaultBand: 0
      },
      GAIN_COLOR,
      { msToX, viewStart, viewEnd },
      highlight
    )
  );
}

export function makeMsToX(view: ViewWindow, trackWidth: number): (ms: number) => number {
  return (ms: number) => LABEL_W + msToFrac(ms, view) * trackWidth;
}

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
  // Drawn last, over scrolled content, so it covers rather than relying on an
  // empty canvas.
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
  accent: string;
  audible: boolean;
  solo: boolean;
  deckLabel: string;
  badgeLabel: string;
  laneLabel: (key: DeckLaneKey) => string;
  badgeAlpha: number;
  openLane: EditableLaneKey | null;
  menuOpen: boolean;
};

// Turned on its side, because the label column is 32px wide and a lane is tall:
// a full word only fits down the height.
function drawRotatedLabel(
  ctx: CanvasRenderingContext2D,
  text: string,
  x: number,
  y: number,
  band: { top: number; height: number }
): void {
  ctx.save();
  ctx.beginPath();
  ctx.rect(0, band.top, LABEL_W, band.height);
  ctx.clip();
  ctx.translate(x, y);
  ctx.rotate(-Math.PI / 2);
  ctx.fillText(text, 0, 0);
  ctx.restore();
}

export function drawDeckRowChrome(
  ctx: CanvasRenderingContext2D,
  row: RowLayout,
  canvasW: number,
  chrome: DeckRowChrome
): void {
  ctx.fillStyle = DECK_ROW_BG_COLOR;
  ctx.fillRect(0, row.top, canvasW, row.height);
  ctx.fillStyle = chrome.accent + DECK_ROW_TINT_ALPHA;
  ctx.fillRect(0, row.top, canvasW, row.height);

  ctx.font = BOLD_LABEL_FONT;
  ctx.fillStyle = chrome.audible ? chrome.accent : DECK_LABEL_INACTIVE_COLOR;
  ctx.textAlign = 'center';
  ctx.textBaseline = 'middle';
  const deckCenterY = row.top + row.waveformHeight / 2;
  const caret = chrome.menuOpen ? LANE_CARET_OPEN : LANE_CARET_CLOSED;
  // Steps aside as the badge fades in, so an unbadged row stays centred.
  const waveformBand = { top: row.top, height: row.waveformHeight };
  drawRotatedLabel(
    ctx,
    `${chrome.deckLabel} ${caret}`,
    LABEL_W / 2 - chrome.badgeAlpha * LANE_LABEL_ROTATED_DX,
    deckCenterY,
    waveformBand
  );
  if (chrome.badgeAlpha > 0) {
    ctx.font = BOLD_SUB_LABEL_FONT;
    ctx.fillStyle = chrome.solo ? SOLO_COLOR : DISABLED_COLOR;
    ctx.globalAlpha = chrome.badgeAlpha;
    drawRotatedLabel(
      ctx,
      chrome.badgeLabel,
      LABEL_W / 2 + SOLO_MUTE_LABEL_OFFSET_PX,
      deckCenterY,
      waveformBand
    );
    ctx.globalAlpha = 1;
  }

  // Each lane's label doubles as a dropdown; Timeline.vue hit-tests this region.
  ctx.fillStyle = chrome.accent;
  ctx.font = BOLD_LABEL_FONT;
  for (const lane of row.lanes) {
    const caret = lane.key === chrome.openLane ? LANE_CARET_OPEN : LANE_CARET_CLOSED;
    drawRotatedLabel(
      ctx,
      `${chrome.laneLabel(lane.key)} ${caret}`,
      LABEL_W / 2,
      lane.top + lane.height / 2,
      lane
    );
  }
}

export type MasterSublane = { key: MasterLaneKey; top: number; height: number };

export function drawMasterRowChrome(
  ctx: CanvasRenderingContext2D,
  top: number,
  height: number,
  canvasW: number,
  sublanes: MasterSublane[],
  laneLabel: (key: MasterLaneKey) => string,
  openLane: MasterLaneKey | null
): void {
  ctx.fillStyle = MASTER_ROW_BG_COLOR;
  ctx.fillRect(0, top, canvasW, height);

  ctx.fillStyle = LANE_BORDER_COLOR;
  for (const lane of sublanes.slice(1)) {
    ctx.fillRect(LABEL_W, lane.top, canvasW - LABEL_W - PADDING, 1);
  }

  // Each lane's label doubles as a dropdown, as on a deck row.
  ctx.font = BOLD_LABEL_FONT;
  ctx.textAlign = 'center';
  ctx.textBaseline = 'middle';
  ctx.fillStyle = LANE_DROPDOWN_COLOR;
  for (const lane of sublanes) {
    const caret = lane.key === openLane ? LANE_CARET_OPEN : LANE_CARET_CLOSED;
    drawRotatedLabel(
      ctx,
      `${laneLabel(lane.key)} ${caret}`,
      LABEL_W / 2,
      lane.top + lane.height / 2,
      lane
    );
  }
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
  // the dividers at extreme values). The label is drawn after, outside the clip.
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
