export function clampToViewport(e: MouseEvent, rect: DOMRect): { x: number; y: number } {
  const x = rect.right > window.innerWidth ? e.clientX - rect.width : e.clientX;
  const y = rect.bottom > window.innerHeight ? e.clientY - rect.height : e.clientY;
  return { x, y };
}
