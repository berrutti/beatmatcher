export type TriangleStyle = {
  fill: string;
  outline: string | null;
  outlineWidth: number;
  alpha: number;
};

// The apex sits at baseY + pointHeight, so a negative height points up out of a bottom edge.
export function drawMarkerTriangle(
  ctx: CanvasRenderingContext2D,
  x: number,
  baseY: number,
  halfWidth: number,
  pointHeight: number,
  style: TriangleStyle
): void {
  ctx.save();
  ctx.globalAlpha = style.alpha;
  ctx.beginPath();
  ctx.moveTo(x - halfWidth, baseY);
  ctx.lineTo(x + halfWidth, baseY);
  ctx.lineTo(x, baseY + pointHeight);
  ctx.closePath();
  ctx.fillStyle = style.fill;
  if (style.outline) {
    ctx.strokeStyle = style.outline;
    ctx.lineWidth = style.outlineWidth;
    ctx.stroke();
  }
  ctx.fill();
  ctx.restore();
}
