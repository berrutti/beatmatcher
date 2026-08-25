// The index a dragged row would occupy, taken from where the row itself would
// sit rather than from where the pointer is. Measuring the pointer against row
// boundaries instead makes the gesture asymmetric: a boundary is crossed after
// one row of travel downwards but a row and a half upwards, so the row lands
// above the point it was picked up by.
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
