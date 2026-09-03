// A canvas cannot read --color-cue, so the value lives here once instead of in each drawer.
const CUE_COLOR = '#eab308';
export const CUE_CHANNELS = '234, 179, 8';

const CUE_ALPHA = 0.9;
const CUE_OUTLINE_WIDTH = 1.5;

export function drawCueTriangle(
  ctx: CanvasRenderingContext2D,
  x: number,
  baseY: number,
  halfWidth: number,
  pointHeight: number,
  outline?: string
): void {
  ctx.save();
  ctx.globalAlpha = CUE_ALPHA;
  ctx.beginPath();
  ctx.moveTo(x - halfWidth, baseY);
  ctx.lineTo(x + halfWidth, baseY);
  ctx.lineTo(x, baseY + pointHeight);
  ctx.closePath();
  ctx.fillStyle = CUE_COLOR;
  if (outline) {
    ctx.strokeStyle = outline;
    ctx.lineWidth = CUE_OUTLINE_WIDTH;
    ctx.stroke();
  }
  ctx.fill();
  ctx.restore();
}
