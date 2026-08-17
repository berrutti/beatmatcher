// Rounded because canvas.width/height are integers but clientWidth/clientHeight can be fractional.
export function computeCanvasSize(
  clientWidth: number,
  clientHeight: number,
  dpr: number
): { width: number; height: number } | null {
  if (!clientWidth || !clientHeight) return null;
  return {
    width: Math.round(clientWidth * dpr),
    height: Math.round(clientHeight * dpr)
  };
}
