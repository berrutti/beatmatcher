import { drawMarkerTriangle } from '@renderer/utils/markerTriangle';

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
  drawMarkerTriangle(ctx, x, baseY, halfWidth, pointHeight, {
    fill: CUE_COLOR,
    outline: outline ?? null,
    outlineWidth: CUE_OUTLINE_WIDTH,
    alpha: CUE_ALPHA
  });
}
