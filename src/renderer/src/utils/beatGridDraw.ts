export const MIN_BEAT_SPACING_PX = 6;
export const MIN_BAR_SPACING_PX = 24;

export type BeatGridWeight = {
  // Device pixels, not CSS lineWidth, since these are fillRect not stroke.
  beatCoreDevicePx: number;
  beatOutlineDevicePx: number;
  beatAlpha: number;
  barCoreDevicePx: number;
  barOutlineDevicePx: number;
  barAlpha: number;
  beatMarkerHalfWidth: number;
  downbeatMarkerHalfWidth: number;
};

// The strip is ten seconds wide and the grid is the beatmatching aid, so it can be heavy.
export const STRIP_GRID: BeatGridWeight = {
  beatCoreDevicePx: 2,
  beatOutlineDevicePx: 4,
  beatAlpha: 0.85,
  barCoreDevicePx: 2,
  barOutlineDevicePx: 6,
  barAlpha: 1,
  beatMarkerHalfWidth: 4,
  downbeatMarkerHalfWidth: 6
};

// The edit view spans minutes and the waveform is the subject, so the grid stays out of its
// way: hairlines, and a triangle only where a bar starts.
export const EDIT_GRID: BeatGridWeight = {
  beatCoreDevicePx: 1,
  beatOutlineDevicePx: 0,
  beatAlpha: 0.18,
  barCoreDevicePx: 1,
  barOutlineDevicePx: 0,
  barAlpha: 0.5,
  beatMarkerHalfWidth: 0,
  downbeatMarkerHalfWidth: 5
};

const MARKER_HEIGHT_RATIO = 1.4;
const BEAT_MARKER_FILL_COLOR = '#ffffff';
const DOWNBEAT_MARKER_FILL_COLOR = 'rgb(220,30,30)';
const LINE_OUTLINE_COLOR = 'rgba(0,0,0,0.7)';
export const MARKER_OUTLINE_COLOR = '#000000';
const MARKER_OUTLINE_WIDTH = 1.5;

// A stroke's anti-aliased edge would shimmer as position scrolls. A pixel-aligned fill can't.
export function fillPixelLine(
  ctx: CanvasRenderingContext2D,
  centerX: number,
  top: number,
  height: number,
  devicePxWidth: number,
  dpr: number,
  color: string
): void {
  const leftDevicePx = Math.round(centerX * dpr) - devicePxWidth / 2;
  ctx.fillStyle = color;
  ctx.fillRect(leftDevicePx / dpr, top, devicePxWidth / dpr, height);
}

function drawGridLine(
  ctx: CanvasRenderingContext2D,
  x: number,
  top: number,
  height: number,
  dpr: number,
  core: number,
  outline: number,
  alpha: number
): void {
  if (outline > 0) fillPixelLine(ctx, x, top, height, outline, dpr, LINE_OUTLINE_COLOR);
  fillPixelLine(ctx, x, top, height, core, dpr, `rgba(255,255,255,${alpha})`);
}

export function drawBeatLine(
  ctx: CanvasRenderingContext2D,
  x: number,
  top: number,
  height: number,
  dpr: number,
  weight: BeatGridWeight
): void {
  drawGridLine(
    ctx,
    x,
    top,
    height,
    dpr,
    weight.beatCoreDevicePx,
    weight.beatOutlineDevicePx,
    weight.beatAlpha
  );
}

export function drawBarLine(
  ctx: CanvasRenderingContext2D,
  x: number,
  top: number,
  height: number,
  dpr: number,
  weight: BeatGridWeight
): void {
  drawGridLine(
    ctx,
    x,
    top,
    height,
    dpr,
    weight.barCoreDevicePx,
    weight.barOutlineDevicePx,
    weight.barAlpha
  );
}

function drawTriangle(
  ctx: CanvasRenderingContext2D,
  x: number,
  baseY: number,
  halfWidth: number,
  pointHeight: number,
  fill: string
): void {
  ctx.beginPath();
  ctx.moveTo(x - halfWidth, baseY);
  ctx.lineTo(x + halfWidth, baseY);
  ctx.lineTo(x, baseY + pointHeight);
  ctx.closePath();
  ctx.fillStyle = fill;
  ctx.strokeStyle = MARKER_OUTLINE_COLOR;
  ctx.lineWidth = MARKER_OUTLINE_WIDTH;
  ctx.stroke();
  ctx.fill();
}

// On both edges so the grid reads without following a line across the waveform.
export function drawBeatMarker(
  ctx: CanvasRenderingContext2D,
  x: number,
  top: number,
  height: number,
  isDownbeat: boolean,
  weight: BeatGridWeight
): void {
  const halfWidth = isDownbeat ? weight.downbeatMarkerHalfWidth : weight.beatMarkerHalfWidth;
  if (halfWidth <= 0) return;
  const pointHeight = halfWidth * MARKER_HEIGHT_RATIO;
  const fill = isDownbeat ? DOWNBEAT_MARKER_FILL_COLOR : BEAT_MARKER_FILL_COLOR;
  ctx.save();
  drawTriangle(ctx, x, top, halfWidth, pointHeight, fill);
  drawTriangle(ctx, x, top + height, halfWidth, -pointHeight, fill);
  ctx.restore();
}
