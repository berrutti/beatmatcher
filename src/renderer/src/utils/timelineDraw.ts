import type {
  Clip,
  LoadedSpan,
  DeckLanes,
  LanePoint,
  NudgeSpan
} from '@renderer/composables/useSessionTimeline';
import { DECK_ACCENTS, DeckId } from '@renderer/stores/decks';
import { EQ_MIN_DB, EQ_MAX_DB } from '@renderer/stores/mixer';
import { formatMs } from '@renderer/utils/time';
import {
  overlapsRange,
  msToFrac,
  sliceVisiblePoints,
  type ViewWindow
} from '@renderer/utils/timelineView';

export type TrackWaveform = { durationSec: number; amps: Float32Array };

export const DECK_ORDER = ['A', 'B', 'C', 'D'] as const;
export const ROW_H = 44;
export const LABEL_W = 32;
export const TICK_H = 16;
export const PADDING = 12;
export const SUBLANE_H = 16;
export const MASTER_ROW_H = SUBLANE_H * 2;
export const OVERVIEW_H = 22;
export const OVERVIEW_GAP = 4;

export const LANE_KEYS = ['gain', 'filter', 'eqLow', 'eqMid', 'eqHigh'] as const;
export type LaneKey = (typeof LANE_KEYS)[number];
export type LaneVisibility = Partial<Record<LaneKey, boolean>>;

export const LANE_GROUP: Record<LaneKey, number> = {
  gain: 0,
  filter: 1,
  eqLow: 2,
  eqMid: 2,
  eqHigh: 2
};
export const LANE_SHORT_LABELS: Record<LaneKey, string> = {
  gain: 'G',
  filter: 'F',
  eqLow: 'LO',
  eqMid: 'MD',
  eqHigh: 'HI'
};

const GAIN_COLOR = '#e5e5e5';
const NUDGE_COLOR = '#fbbf24';
const NUDGE_LINE_W = 2;
const FILTER_DEAD_ZONE = 0.05;
const FILTER_LPF_COLOR = '#38bdf8';
const FILTER_HPF_COLOR = '#fb923c';
const FILTER_NEUTRAL_COLOR = '#666666';
const FILTER_ACTIVE_FILL = '#ffffff10';
const EQ_BAND_COLORS: Record<string, string> = { low: '#ef4444', mid: '#eab308', high: '#3b82f6' };

export type RowLayout = { deckId: DeckId; top: number; height: number; lanes: LaneKey[] };
export type OverviewRect = { y: number; h: number };

export function formatTickLabel(ms: number, tickIntervalMs: number): string {
  if (tickIntervalMs < 1000) {
    const totalSec = Math.floor(ms / 1000);
    const mins = Math.floor(totalSec / 60);
    const secs = totalSec % 60;
    const millis = ms % 1000;
    return `${mins}:${String(secs).padStart(2, '0')}:${String(millis).padStart(3, '0')}`;
  }
  return formatMs(ms);
}

export function valueToY(
  laneY: number,
  laneHeight: number,
  minVal: number,
  maxVal: number,
  value: number
): number {
  const range = maxVal - minVal || 1;
  return laneY + laneHeight - ((value - minVal) / range) * laneHeight;
}

export function filterColorFor(value: number): string {
  if (value < -FILTER_DEAD_ZONE) return FILTER_LPF_COLOR;
  if (value > FILTER_DEAD_ZONE) return FILTER_HPF_COLOR;
  return FILTER_NEUTRAL_COLOR;
}

export function drawClipWaveform(
  ctx: CanvasRenderingContext2D,
  clip: Clip,
  waveform: TrackWaveform | undefined,
  rectX: number,
  rectY: number,
  rectWidth: number,
  rectHeight: number,
  accent: string
): void {
  if (rectWidth < 2 || !waveform || waveform.amps.length === 0 || waveform.durationSec <= 0) return;

  const wallDurationSec = (clip.sessionEndMs - clip.sessionStartMs) / 1000;
  const trackEndSec = clip.trackStartSec + wallDurationSec * clip.playbackRate;
  const startFraction = clip.trackStartSec / waveform.durationSec;
  const endFraction = Math.min(1, trackEndSec / waveform.durationSec);
  if (endFraction <= startFraction) return;

  const numPoints = waveform.amps.length;
  const centerY = rectY + rectHeight / 2;
  const maxBarHalf = rectHeight * 0.45;

  ctx.save();
  ctx.beginPath();
  ctx.rect(rectX, rectY, rectWidth, rectHeight);
  ctx.clip();
  ctx.fillStyle = accent + 'aa';

  const columnCount = Math.ceil(rectWidth);
  for (let column = 0; column < columnCount; column++) {
    const columnFraction = column / rectWidth;
    const trackFraction = startFraction + columnFraction * (endFraction - startFraction);
    const sourceStart = trackFraction * numPoints;
    const sourceEnd =
      (startFraction + ((column + 1) / rectWidth) * (endFraction - startFraction)) * numPoints;
    const indexStart = sourceStart | 0;
    const indexEnd = Math.min(numPoints - 1, Math.max(indexStart, (sourceEnd - 1e-9) | 0));

    let amplitudeSum = 0;
    let sampleCount = 0;
    for (let idx = indexStart; idx <= indexEnd; idx++) {
      amplitudeSum += waveform.amps[idx];
      sampleCount++;
    }
    const avgAmplitude = sampleCount > 0 ? amplitudeSum / sampleCount : 0;
    const barHeight = Math.max(1, Math.sqrt(avgAmplitude) * maxBarHalf);
    ctx.fillRect(rectX + column, centerY - barHeight, 1, barHeight * 2);
  }

  ctx.restore();
}

// Draws a zero-order-hold (step) graph: each point's value is held flat until
// the next point's time, then jumps. This matches how parameters actually change:
// session events fire at the moment a value changes, not gradually beforehand.
export function drawLaneSteps(
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

export function drawFilterLane(
  ctx: CanvasRenderingContext2D,
  canvasWidth: number,
  deckData: DeckLanes,
  laneY: number,
  msToX: (ms: number) => number,
  viewStart: number,
  viewEnd: number
): void {
  for (const span of deckData.filterActive) {
    if (!overlapsRange(span.startMs, span.endMs, viewStart, viewEnd)) continue;
    const spanX = msToX(span.startMs);
    const spanWidth = Math.max(1, msToX(span.endMs) - spanX);
    ctx.fillStyle = FILTER_ACTIVE_FILL;
    ctx.fillRect(spanX, laneY, spanWidth, SUBLANE_H);
  }

  // Center line marks the bypass position (knob = 0); LPF sweeps below it, HPF above.
  const centerY = valueToY(laneY, SUBLANE_H, -1, 1, 0);
  ctx.strokeStyle = '#3a3a3a';
  ctx.beginPath();
  ctx.moveTo(LABEL_W, centerY);
  ctx.lineTo(canvasWidth - PADDING, centerY);
  ctx.stroke();

  drawLaneSteps(
    ctx,
    deckData.filter,
    laneY,
    SUBLANE_H,
    -1,
    1,
    filterColorFor,
    msToX,
    viewStart,
    viewEnd
  );
}

export function drawEqLane(
  ctx: CanvasRenderingContext2D,
  points: LanePoint[],
  color: string,
  laneY: number,
  msToX: (ms: number) => number,
  viewStart: number,
  viewEnd: number
): void {
  drawLaneSteps(
    ctx,
    points,
    laneY,
    SUBLANE_H,
    EQ_MIN_DB,
    EQ_MAX_DB,
    color,
    msToX,
    viewStart,
    viewEnd
  );
}

export type LaneDrawer = (
  ctx: CanvasRenderingContext2D,
  canvasWidth: number,
  deckData: DeckLanes,
  laneY: number,
  msToX: (ms: number) => number,
  viewStart: number,
  viewEnd: number
) => void;

export const LANE_DRAWERS: Record<LaneKey, LaneDrawer> = {
  gain: (ctx, _w, deckData, laneY, msToX, viewStart, viewEnd) =>
    drawLaneSteps(
      ctx,
      deckData.gain,
      laneY,
      SUBLANE_H,
      0,
      1,
      GAIN_COLOR,
      msToX,
      viewStart,
      viewEnd
    ),
  filter: drawFilterLane,
  eqLow: (ctx, _w, deckData, laneY, msToX, viewStart, viewEnd) =>
    drawEqLane(ctx, deckData.eqLow, EQ_BAND_COLORS.low, laneY, msToX, viewStart, viewEnd),
  eqMid: (ctx, _w, deckData, laneY, msToX, viewStart, viewEnd) =>
    drawEqLane(ctx, deckData.eqMid, EQ_BAND_COLORS.mid, laneY, msToX, viewStart, viewEnd),
  eqHigh: (ctx, _w, deckData, laneY, msToX, viewStart, viewEnd) =>
    drawEqLane(ctx, deckData.eqHigh, EQ_BAND_COLORS.high, laneY, msToX, viewStart, viewEnd)
};

export function drawDeckLanes(
  ctx: CanvasRenderingContext2D,
  canvasWidth: number,
  laneTopY: number,
  msToX: (ms: number) => number,
  deckData: DeckLanes | undefined,
  lanes: LaneKey[],
  viewStart: number,
  viewEnd: number
): void {
  if (!deckData) return;

  let laneY = laneTopY;
  for (let laneIdx = 0; laneIdx < lanes.length; laneIdx++) {
    const group = LANE_GROUP[lanes[laneIdx]];
    const prevGroup = laneIdx > 0 ? LANE_GROUP[lanes[laneIdx - 1]] : -1;

    ctx.fillStyle = group % 2 === 0 ? '#1a1a1a' : '#141414';
    ctx.fillRect(LABEL_W, laneY, canvasWidth - LABEL_W - PADDING, SUBLANE_H);

    if (laneIdx > 0 && group !== prevGroup) {
      ctx.fillStyle = '#2a2a2a';
      ctx.fillRect(LABEL_W, laneY, canvasWidth - LABEL_W - PADDING, 1);
    }

    LANE_DRAWERS[lanes[laneIdx]](ctx, canvasWidth, deckData, laneY, msToX, viewStart, viewEnd);
    laneY += SUBLANE_H;
  }
}

export function drawLoadedSpan(
  ctx: CanvasRenderingContext2D,
  span: LoadedSpan,
  rowY: number,
  accent: string,
  msToX: (ms: number) => number
): void {
  const spanX = msToX(span.startMs);
  const spanWidth = Math.max(2, msToX(span.endMs) - spanX);
  const spanY = rowY + 4;
  const spanHeight = ROW_H - 8;

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
  msToX: (ms: number) => number
): void {
  const spanX = msToX(span.startMs);
  const spanWidth = Math.max(2, msToX(span.endMs) - spanX);
  if (spanWidth <= 40) return;
  const spanY = rowY + 4;
  const spanHeight = ROW_H - 8;
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
  accent: string,
  msToX: (ms: number) => number
): void {
  const clipX = msToX(clip.sessionStartMs);
  const clipWidth = Math.max(2, msToX(clip.sessionEndMs) - clipX);
  const clipY = rowY + 4;
  const clipHeight = ROW_H - 8;

  ctx.fillStyle = accent + '55';
  ctx.fillRect(clipX, clipY, clipWidth, clipHeight);

  drawClipWaveform(ctx, clip, waveform, clipX, clipY, clipWidth, clipHeight, accent);

  ctx.strokeStyle = accent + 'cc';
  ctx.lineWidth = 1;
  ctx.strokeRect(clipX + 0.5, clipY + 0.5, clipWidth - 1, clipHeight - 1);
}

export function drawNudgeSpans(
  ctx: CanvasRenderingContext2D,
  nudgeSpans: NudgeSpan[],
  rowY: number,
  msToX: (ms: number) => number
): void {
  if (nudgeSpans.length === 0) return;
  const innerY = rowY + 4;
  const innerHeight = ROW_H - 8;
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
  msToX: (ms: number) => number,
  viewStart: number,
  viewEnd: number
): void {
  drawLaneSteps(
    ctx,
    points,
    masterTopY + 2,
    MASTER_ROW_H - 4,
    0,
    1,
    GAIN_COLOR,
    msToX,
    viewStart,
    viewEnd
  );
}

export function makeMsToX(view: ViewWindow, trackWidth: number): (ms: number) => number {
  return (ms: number) => LABEL_W + msToFrac(ms, view) * trackWidth;
}
