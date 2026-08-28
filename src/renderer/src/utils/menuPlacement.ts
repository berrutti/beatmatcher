export const MENU_VIEWPORT_MARGIN_PX = 8;

function clampAxis(start: number, extent: number, available: number): number {
  return Math.max(
    MENU_VIEWPORT_MARGIN_PX,
    Math.min(start, available - extent - MENU_VIEWPORT_MARGIN_PX)
  );
}

export function clampMenuPosition(
  desired: { x: number; y: number },
  size: { width: number; height: number },
  viewport: { width: number; height: number }
): { x: number; y: number } {
  return {
    x: clampAxis(desired.x, size.width, viewport.width),
    y: clampAxis(desired.y, size.height, viewport.height)
  };
}
