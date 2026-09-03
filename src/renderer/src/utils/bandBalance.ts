export type BandSquares = [number, number, number];

export const FLAT_BALANCE: BandSquares = [1, 1, 1];

export function addBandSquares(into: BandSquares, points: Float32Array, count: number): void {
  for (let point = 0; point < count; point++) {
    const at = point * 4;
    into[0] += points[at] * points[at];
    into[1] += points[at + 1] * points[at + 1];
    into[2] += points[at + 2] * points[at + 2];
  }
}

// Each band lifted to the track's own level, which is the three levels in quadrature. The
// same rule Rust scales the emitted values by, so the two agree on what neutral means.
export function bandBalanceOf(squares: BandSquares, count: number): BandSquares {
  if (count <= 0) return [...FLAT_BALANCE];
  const levels = squares.map((sum) => Math.sqrt(sum / count));
  const reference = Math.sqrt(levels.reduce((total, level) => total + level * level, 0));
  const balance = levels.map((level) => (level > 0 ? reference / level : 1));
  return [balance[0], balance[1], balance[2]];
}

// A chunk destined for a buffer sized by a previous, shorter track would write past its
// end; the caller reallocates instead of letting `set` throw mid-load.
export function densePointsFit(
  buffer: Float32Array | null,
  fromPoint: number,
  arrivedPoints: number
): boolean {
  if (!buffer) return false;
  return (fromPoint + arrivedPoints) * 4 <= buffer.length;
}
