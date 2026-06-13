import type {
  Clip,
  LoadedSpan,
  DeckLanes,
  LanePoint,
  NudgeSpan
} from '@renderer/composables/useSessionTimeline';
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

export const LANE_KEYS = ['gain', 'filter', 'rate', 'eqLow', 'eqMid', 'eqHigh'] as const;
export type LaneKey = (typeof LANE_KEYS)[number];
export type LaneVisibility = Partial<Record<LaneKey, boolean>>;

export const LANE_GROUP: Record<LaneKey, number> = {
  gain: 0,
  filter: 1,
  rate: 2,
  eqLow: 3,
  eqMid: 3,
  eqHigh: 3
};
export const LANE_SHORT_LABELS: Record<LaneKey, string> = {
  gain: 'G',
  filter: 'F',
  rate: 'RT',
  eqLow: 'LO',
  eqMid: 'MD',
  eqHigh: 'HI'
};

const GAIN_COLOR = '#e5e5e5';
const RATE_COLOR = '#a78bfa';
const NUDGE_COLOR = '#fbbf24';
const NUDGE_LINE_W = 2;
const FILTER_LPF_COLOR = '#38bdf8';
const FILTER_HPF_COLOR = '#fb923c';
const FILTER_NEUTRAL_COLOR = '#666666';
const FILTER_ACTIVE_FILL = '#ffffff10';
const EQ_BAND_COLORS: Record<string, string> = { low: '#ef4444', mid: '#eab308', high: '#3b82f6' };

export type SublaneLayout = { key: LaneKey; top: number; height: number };
export type RowLayout = { deckId: DeckId; top: number; height: number; lanes: SublaneLayout[] };
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

  // Iterate only the columns inside the visible track area: when zoomed in,
  // a clip can be tens of thousands of pixels wide and drawing the offscreen
  // columns froze the UI.
  const visibleLeft = Math.max(rectX, LABEL_W);
  const visibleRight = Math.min(rectX + rectWidth, ctx.canvas.clientWidth - PADDING);
  const columnStart = Math.max(0, Math.floor(visibleLeft - rectX));
  const columnCount = Math.min(Math.ceil(rectWidth), Math.ceil(visibleRight - rectX));
  for (let column = columnStart; column < columnCount; column++) {
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
  ctx.strokeStyle = '#3a3a3a';
  ctx.beginPath();
  ctx.moveTo(LABEL_W, centerY);
  ctx.lineTo(canvasWidth - PADDING, centerY);
  ctx.stroke();
}

export function drawRateLane(
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

export function drawEqLane(
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

export type LaneDrawer = (
  ctx: CanvasRenderingContext2D,
  canvasWidth: number,
  deckData: DeckLanes,
  laneY: number,
  laneH: number,
  msToX: (ms: number) => number,
  viewStart: number,
  viewEnd: number
) => void;

export const LANE_DRAWERS: Record<LaneKey, LaneDrawer> = {
  gain: (ctx, _w, deckData, laneY, laneH, msToX, viewStart, viewEnd) =>
    drawLaneSteps(ctx, deckData.gain, laneY, laneH, 0, 1, GAIN_COLOR, msToX, viewStart, viewEnd),
  filter: drawFilterLane,
  rate: drawRateLane,
  eqLow: (ctx, _w, deckData, laneY, laneH, msToX, viewStart, viewEnd) =>
    drawEqLane(ctx, deckData.eqLow, EQ_BAND_COLORS.low, laneY, laneH, msToX, viewStart, viewEnd),
  eqMid: (ctx, _w, deckData, laneY, laneH, msToX, viewStart, viewEnd) =>
    drawEqLane(ctx, deckData.eqMid, EQ_BAND_COLORS.mid, laneY, laneH, msToX, viewStart, viewEnd),
  eqHigh: (ctx, _w, deckData, laneY, laneH, msToX, viewStart, viewEnd) =>
    drawEqLane(ctx, deckData.eqHigh, EQ_BAND_COLORS.high, laneY, laneH, msToX, viewStart, viewEnd)
};

export function drawDeckLanes(
  ctx: CanvasRenderingContext2D,
  canvasWidth: number,
  msToX: (ms: number) => number,
  deckData: DeckLanes | undefined,
  sublanes: SublaneLayout[],
  viewStart: number,
  viewEnd: number
): void {
  if (!deckData) return;

  for (let laneIdx = 0; laneIdx < sublanes.length; laneIdx++) {
    const { key, top, height } = sublanes[laneIdx];
    const group = LANE_GROUP[key];
    const prevGroup = laneIdx > 0 ? LANE_GROUP[sublanes[laneIdx - 1].key] : -1;

    ctx.fillStyle = group % 2 === 0 ? '#1a1a1a' : '#141414';
    ctx.fillRect(LABEL_W, top, canvasWidth - LABEL_W - PADDING, height);

    if (laneIdx > 0 && group !== prevGroup) {
      ctx.fillStyle = '#2a2a2a';
      ctx.fillRect(LABEL_W, top, canvasWidth - LABEL_W - PADDING, 1);
    }

    LANE_DRAWERS[key](ctx, canvasWidth, deckData, top, height, msToX, viewStart, viewEnd);
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

// White so it stays visible over any user-chosen deck accent color.
export function drawClipSelection(
  ctx: CanvasRenderingContext2D,
  startMs: number,
  endMs: number,
  rowY: number,
  msToX: (ms: number) => number
): void {
  const x = msToX(startMs);
  const width = Math.max(2, msToX(endMs) - x);
  const y = rowY + 4;
  const height = ROW_H - 8;
  ctx.fillStyle = 'rgba(255, 255, 255, 0.14)';
  ctx.fillRect(x, y, width, height);
  ctx.strokeStyle = 'rgba(255, 255, 255, 0.9)';
  ctx.lineWidth = 1;
  ctx.strokeRect(x + 0.5, y + 0.5, width - 1, height - 1);
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
  masterRowH: number,
  msToX: (ms: number) => number,
  viewStart: number,
  viewEnd: number
): void {
  drawLaneSteps(
    ctx,
    points,
    masterTopY + 2,
    masterRowH - 4,
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

// ── draw() orchestration pieces ──────────────────────────────────────────────
// Renderers extracted from Timeline.vue's draw(): each takes the context plus
// explicit data, no component state, so draw() stays a short orchestrator.

export function drawOutlinedLabel(
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
  selectedLaneKey: LaneKey | null;
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
  ctx.fillStyle = chrome.audible ? chrome.accent : '#555';
  ctx.textAlign = 'center';
  ctx.textBaseline = 'middle';
  ctx.fillText(row.deckId, LABEL_W / 2, row.top + ROW_H / 2);
  if (chrome.solo || chrome.muted) {
    ctx.font = `bold 7px monospace`;
    ctx.fillStyle = chrome.solo ? '#eab308' : '#ef4444';
    ctx.fillText(chrome.solo ? 'S' : 'M', LABEL_W / 2, row.top + ROW_H / 2 + 11);
  }

  if (row.lanes.length > 0) {
    ctx.font = `8px monospace`;
    for (const sublane of row.lanes) {
      ctx.fillStyle = sublane.key === chrome.selectedLaneKey ? '#06b6d4' : '#555';
      ctx.fillText(LANE_SHORT_LABELS[sublane.key], LABEL_W / 2, sublane.top + sublane.height / 2);
    }
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
  ctx.fillStyle = '#888';
  ctx.textAlign = 'center';
  ctx.textBaseline = 'middle';
  ctx.fillText('M', LABEL_W / 2, top + height / 2);
}

export type DeckRowContent = {
  accent: string;
  loadedSpans: LoadedSpan[];
  clips: Clip[];
  waveforms: Map<string, TrackWaveform>;
  nudgeSpans: NudgeSpan[];
  deckLanes: DeckLanes | undefined;
  audible: boolean;
  selectionSpan: { startMs: number; endMs: number } | null;
};

export function drawDeckRowContent(
  ctx: CanvasRenderingContext2D,
  row: RowLayout,
  content: DeckRowContent,
  canvasW: number,
  trackW: number,
  view: ViewWindow,
  msToX: (ms: number) => number
): void {
  const viewEnd = view.start + view.duration;
  const visible = (startMs: number, endMs: number) =>
    overlapsRange(startMs, endMs, view.start, viewEnd);

  for (const span of content.loadedSpans) {
    if (span.deck === row.deckId && visible(span.startMs, span.endMs)) {
      drawLoadedSpan(ctx, span, row.top, content.accent, msToX);
    }
  }

  for (const clip of content.clips) {
    if (clip.deck === row.deckId && visible(clip.sessionStartMs, clip.sessionEndMs)) {
      drawClip(ctx, clip, content.waveforms.get(clip.trackPath), row.top, content.accent, msToX);
    }
  }

  drawNudgeSpans(
    ctx,
    content.nudgeSpans.filter((span) => visible(span.startMs, span.endMs)),
    row.top,
    msToX
  );

  if (row.lanes.length > 0) {
    drawDeckLanes(ctx, canvasW, msToX, content.deckLanes, row.lanes, view.start, viewEnd);
  }

  if (!content.audible) {
    ctx.fillStyle = '#00000090';
    ctx.fillRect(LABEL_W, row.top, trackW, ROW_H);
  }

  if (content.selectionSpan && visible(content.selectionSpan.startMs, content.selectionSpan.endMs)) {
    drawClipSelection(ctx, content.selectionSpan.startMs, content.selectionSpan.endMs, row.top, msToX);
  }
}

export function drawSelectedLaneHighlight(
  ctx: CanvasRenderingContext2D,
  rect: { top: number; height: number },
  trackW: number
): void {
  ctx.fillStyle = '#06b6d414';
  ctx.fillRect(LABEL_W, rect.top, trackW, rect.height);
  ctx.strokeStyle = '#06b6d4';
  ctx.lineWidth = 1;
  ctx.strokeRect(LABEL_W + 0.5, rect.top + 0.5, trackW - 1, rect.height - 1);
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

  const labelX = Math.min(msToX(labelMs) + 8, canvasW - PADDING - 40);
  drawOutlinedLabel(ctx, label, labelX, preview.top - 6);
}

export function drawNudgeGesturePreview(
  ctx: CanvasRenderingContext2D,
  t0: number,
  t1: number,
  percent: number,
  rowTop: number,
  cursorMs: number,
  msToX: (ms: number) => number,
  canvasW: number
): void {
  drawNudgeSpans(ctx, [{ startMs: t0, endMs: t1, percent }], rowTop, msToX);
  const label = `${percent > 0 ? '+' : ''}${percent}%`;
  const labelX = Math.min(msToX(cursorMs) + 8, canvasW - PADDING - 30);
  const labelY = percent > 0 ? rowTop + 12 : rowTop + ROW_H - 6;
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
    ctx.fillRect(ghostX, rowTop, ghostW, ROW_H);
    ctx.strokeRect(ghostX + 0.5, rowTop + 0.5, ghostW - 1, ROW_H - 1);
  }
  const labelX = Math.min(msToX(labelMs) + 8, canvasW - PADDING - 60);
  drawOutlinedLabel(ctx, label, labelX, rowTop + 12);
}

export function drawLoadedSpanLabels(
  ctx: CanvasRenderingContext2D,
  rows: RowLayout[],
  loadedSpans: LoadedSpan[],
  view: ViewWindow,
  msToX: (ms: number) => number
): void {
  const viewEnd = view.start + view.duration;
  for (const row of rows) {
    for (const span of loadedSpans) {
      if (span.deck === row.deckId && overlapsRange(span.startMs, span.endMs, view.start, viewEnd)) {
        drawLoadedSpanLabel(ctx, span, row.top, msToX);
      }
    }
  }
}

// Deck row dividers, drawn full-width (including the label column) after the
// track clip is restored. Deliberately heavier than the 1px sublane-group
// separators so deck boundaries stand out: a dark gap with a bright hairline.
export function drawRowDividers(ctx: CanvasRenderingContext2D, rows: RowLayout[], canvasW: number): void {
  for (const row of rows) {
    const dividerY = row.top + row.height - 3;
    ctx.fillStyle = '#000';
    ctx.fillRect(0, dividerY, canvasW, 3);
    ctx.fillStyle = '#5a5a5a';
    ctx.fillRect(0, dividerY, canvasW, 1);
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
