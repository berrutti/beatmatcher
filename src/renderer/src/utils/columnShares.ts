// A share is unitless: a column's width is its share over the sum of the visible
// ones, so they fill the space given to them whatever it is.
export function shareFractions<F extends string>(
  fields: F[],
  getShare: (field: F) => number
): Record<F, number> {
  const total = fields.reduce((sum, field) => sum + getShare(field), 0);
  const result = {} as Record<F, number>;
  for (const field of fields) {
    result[field] = total > 0 ? getShare(field) / total : 0;
  }
  return result;
}

type ResizeShareDeltaOptions<F extends string> = {
  fields: F[];
  getShare: (field: F) => number;
  field: F;
  neighbor: F;
  deltaPx: number;
  availableWidth: number;
  minPx: number;
};

// `deltaPx` is movement since the last call, not since the drag started, so a
// drag needs nothing snapshotted up front.
export function resizeShareDelta<F extends string>(
  options: ResizeShareDeltaOptions<F>
): { field: number; neighbor: number } {
  const { fields, getShare, field, neighbor, deltaPx, availableWidth, minPx } = options;
  const total = fields.reduce((sum, f) => sum + getShare(f), 0);
  const deltaShare = availableWidth > 0 ? (deltaPx / availableWidth) * total : 0;
  const pairShare = getShare(field) + getShare(neighbor);
  const minShare = availableWidth > 0 ? (minPx / availableWidth) * total : 0;
  // When the pair doesn't hold enough combined share to give both sides the
  // minimum, split it down the middle instead of producing an inverted
  // clamp range.
  const lo = Math.min(minShare, pairShare / 2);
  const hi = Math.max(pairShare - minShare, pairShare / 2);
  const newFieldShare = Math.min(Math.max(getShare(field) + deltaShare, lo), hi);
  return { field: newFieldShare, neighbor: pairShare - newFieldShare };
}
