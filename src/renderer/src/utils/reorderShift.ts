// The rows a drag passes slide into the gap it left, so the list previews the
// result rather than drawing a line at it. In row heights.
export function reorderShift(index: number, fromIdx: number, targetIdx: number): number {
  if (targetIdx === fromIdx) return 0;
  if (index === fromIdx) return targetIdx - fromIdx;
  if (targetIdx > fromIdx && index > fromIdx && index <= targetIdx) return -1;
  if (targetIdx < fromIdx && index >= targetIdx && index < fromIdx) return 1;
  return 0;
}
