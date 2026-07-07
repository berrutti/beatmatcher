// Resizable columns keep their configured width as a minimum, but grow to
// fill any space left over in a container beyond the sum of their configured
// widths and the table's fixed-width columns, each getting a share
// proportional to its own configured width.
export function distributeColumnWidths<F extends string>(
  containerWidth: number,
  fixedTotal: number,
  fields: F[],
  getWidth: (field: F) => number
): Record<F, number> {
  const result = {} as Record<F, number>;
  const basis = fields.reduce((sum, field) => sum + getWidth(field), 0);
  const extra = Math.max(0, containerWidth - fixedTotal - basis);
  for (const field of fields) {
    const width = getWidth(field);
    const share = basis > 0 ? width / basis : fields.length > 0 ? 1 / fields.length : 0;
    result[field] = width + extra * share;
  }
  return result;
}
