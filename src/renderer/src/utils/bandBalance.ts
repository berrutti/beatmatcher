export type BandSquares = [number, number, number];

const FLAT_BALANCE: BandSquares = [1, 1, 1];

export function addBandSquares(into: BandSquares, points: Float32Array, count: number): void {
  for (let point = 0; point < count; point++) {
    const at = point * 4;
    into[0] += points[at] * points[at];
    into[1] += points[at + 1] * points[at + 1];
    into[2] += points[at + 2] * points[at + 2];
  }
}

function bandLevels([bassSquares, midSquares, highSquares]: BandSquares, count: number) {
  const level = (sum: number) => Math.sqrt(sum / count);
  return { bass: level(bassSquares), mid: level(midSquares), high: level(highSquares) };
}

export function bandReferenceOf(squares: BandSquares, count: number): number {
  if (count <= 0) return 1;
  const { bass, mid, high } = bandLevels(squares, count);
  const reference = Math.sqrt(bass * bass + mid * mid + high * high);
  return reference > 0 ? reference : 1;
}

export function bandBalanceOf(squares: BandSquares, count: number): BandSquares {
  if (count <= 0) return [...FLAT_BALANCE];
  const reference = bandReferenceOf(squares, count);
  const { bass, mid, high } = bandLevels(squares, count);
  const lift = (level: number) => (level > 0 ? reference / level : 1);
  return [lift(bass), lift(mid), lift(high)];
}

// A chunk destined for a buffer sized by a previous, shorter track would write past its
// end; the caller drops it instead of letting `set` throw mid-load.
export function densePointsFit(
  buffer: Float32Array | null,
  fromPoint: number,
  arrivedPoints: number
): buffer is Float32Array {
  if (!buffer) return false;
  return (fromPoint + arrivedPoints) * 4 <= buffer.length;
}

// A move too small to see is not worth repainting every column for.
export function sameBandBalance(a: BandSquares, b: BandSquares): boolean {
  return a.every((value, band) => Math.abs(value - b[band]) < 0.01);
}
