// Shared by every loop-region drawer (mixer/Waveform.vue, EditWaveform.vue,
// TrackWaveform.vue), which each clamped a region to its visible width the
// same way. Each caller keeps its own sec-to-px convention and passes it in
// as `xFor`, so the drawers don't need a shared coordinate system.
export function loopRegionRect(
  xFor: (sec: number) => number,
  region: { startSec: number; endSec: number } | null,
  width: number
): { startX: number; endX: number } | null {
  if (!region) return null;
  const startX = Math.max(0, xFor(region.startSec));
  const endX = Math.min(width, xFor(region.endSec));
  if (endX <= startX) return null;
  return { startX, endX };
}

const LOOP_ACTIVE_COLOR = '#ca8a04';
const LOOP_ARMED_COLOR = '#78716c';
// Active is a strong overlay (it's live audio right now). Armed is a dim hint
// of what reloop would jump into.
const LOOP_ACTIVE_FILL_ALPHA = 0.42;
const LOOP_ARMED_FILL_ALPHA = 0.2;
const LOOP_ACTIVE_STROKE_ALPHA = 0.9;
const LOOP_ARMED_STROKE_ALPHA = 0.5;
const LOOP_STROKE_WIDTH = 1.5;

// The ctx-drawing half, kept separate from loopRegionRect's geometry so the
// geometry stays unit-testable without a canvas (see timelineDraw.ts's
// drawers, which are the same shape: no logic left to assert on here).
export function drawLoopRegionOverlay(
  ctx: CanvasRenderingContext2D,
  rect: { startX: number; endX: number },
  y0: number,
  height: number,
  active: boolean
): void {
  const color = active ? LOOP_ACTIVE_COLOR : LOOP_ARMED_COLOR;
  ctx.save();
  ctx.fillStyle = color;
  ctx.globalAlpha = active ? LOOP_ACTIVE_FILL_ALPHA : LOOP_ARMED_FILL_ALPHA;
  ctx.fillRect(rect.startX, y0, rect.endX - rect.startX, height);
  ctx.globalAlpha = active ? LOOP_ACTIVE_STROKE_ALPHA : LOOP_ARMED_STROKE_ALPHA;
  ctx.strokeStyle = color;
  ctx.lineWidth = LOOP_STROKE_WIDTH;
  ctx.beginPath();
  ctx.moveTo(rect.startX, y0);
  ctx.lineTo(rect.startX, y0 + height);
  ctx.moveTo(rect.endX, y0);
  ctx.lineTo(rect.endX, y0 + height);
  ctx.stroke();
  ctx.restore();
}
