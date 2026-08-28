// Measured from where the row would sit, not the pointer: against row boundaries
// the gesture is asymmetric, crossing after one row down but one and a half up.
export function targetIndexAt(
  clientY: number,
  grabOffsetY: number,
  contentTop: number,
  rowHeight: number,
  rowCount: number
): number {
  if (rowHeight <= 0) return 0;
  const rowTop = clientY - grabOffsetY - contentTop;
  const index = Math.round(rowTop / rowHeight);
  return Math.min(Math.max(index, 0), Math.max(rowCount - 1, 0));
}
