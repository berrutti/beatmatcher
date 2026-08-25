// While a row is being dragged, the row itself travels to the index it would
// land on and the rows it passes slide into the gap it left, so the list
// previews the result rather than drawing a line at it. Returns how many row
// heights row `index` moves.
export function reorderShift(index: number, fromIdx: number, targetIdx: number): number {
  if (targetIdx === fromIdx) return 0;
  if (index === fromIdx) return targetIdx - fromIdx;
  if (targetIdx > fromIdx && index > fromIdx && index <= targetIdx) return -1;
  if (targetIdx < fromIdx && index >= targetIdx && index < fromIdx) return 1;
  return 0;
}
