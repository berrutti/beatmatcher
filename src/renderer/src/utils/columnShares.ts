// Each resizable column holds a unitless positive "share" rather than a
// pixel width. A column's displayed width is always its share divided by
// the sum of every visible resizable column's share - so the visible
// resizable columns always exactly fill whatever space they're given, by
// construction. Rendering turns a fraction into a CSS `calc()` expression
// against the space left over after the fixed columns (see Browser.vue), so
// the browser's own layout engine re-divides that space on every window
// resize with no JS involved; only an actual share change (a drag, or a
// column being hidden/shown) needs to run any of this.
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

// Dragging the handle between `field` and `neighbor` moves share from one to
// the other, keeping their combined share (and therefore every other
// column's own share of the total) exactly constant - a resize can only
// ever trade width with the one column immediately next to it. `deltaPx` is
// the pixel movement since the *last* call (not since the drag started) and
// `getShare` should read the current live share, so each call is a small,
// self-contained step applied on top of whatever the previous call left
// behind - nothing needs to be snapshotted at drag start.
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
