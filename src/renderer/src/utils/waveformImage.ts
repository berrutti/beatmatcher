// Rendering approach inspired by Mixxx (https://github.com/mixxxdj/mixxx):
// mean-in-window pixel aggregation for stable, LOD-aware waveform display.

// On the mixer strip the bars clamp, so brightness carries the envelope alone.
const COLOR_FLOOR = 0.3;

const SILENT_AMP = 0.001;

// Hue on its own made every column fully saturated, which read as confetti, not a shape.
export function spectralColor(
  bass: number,
  mid: number,
  high: number,
  amp: number
): [number, number, number] {
  const strongest = Math.max(bass, mid, high);
  if (strongest <= 0) return [0, 0, 0];

  const brightness = COLOR_FLOOR + (1 - COLOR_FLOOR) * Math.sqrt(Math.min(1, Math.max(0, amp)));
  const scale = (brightness / strongest) * 255;

  return [Math.round(bass * scale), Math.round(mid * scale), Math.round(high * scale)];
}

export type Rgb = [number, number, number];

export const WAVEFORM_BACKGROUND: Rgb = [10, 10, 10];

export type WaveformColumn = { bass: number; mid: number; high: number; amp: number };

// Overlaps are looked up, not mixed. bassMid is darker than either band alone, which no
// additive blend produces, and that dark under the pale midHigh is the whole look.
export type StackedPalette = {
  bass: Rgb;
  mid: Rgb;
  high: Rgb;
  bassMid: Rgb;
  bassHigh: Rgb;
  midHigh: Rgb;
  all: Rgb;
};

// Blended reads each band against its own track average. Stacked reads them against
// each other.
export type WaveformPalette =
  | { kind: 'blended'; balance: [number, number, number] }
  | { kind: 'stacked'; colors: StackedPalette };

export type WaveformPaint = {
  ampScale: number;
  // A band level of 1 is the track's own average across the three bands, so this is where
  // average content sits and how much headroom a kick has left.
  bandScale: number;
  // Leaves room for the beat grid.
  maxBarFraction: number;
  background: Rgb | null;
  // Drawn where a column has nothing to show, so a track still being analysed reads as a
  // line waiting to be filled rather than as a hole.
  baseline: Rgb | null;
  palette: WaveformPalette;
};

export function waveformColumns(
  peaks: Float32Array,
  columnCount: number,
  fromPoint = 0,
  toPoint = (peaks.length / 4) | 0,
  bandReference = 1
): WaveformColumn[] {
  const columns: WaveformColumn[] = [];
  const pointCount = Math.max(0, toPoint - fromPoint);
  if (columnCount <= 0 || pointCount === 0) return columns;

  for (let column = 0; column < columnCount; column++) {
    const srcStart = (column * pointCount) / columnCount;
    const srcEnd = ((column + 1) * pointCount) / columnCount;
    const first = srcStart | 0;
    const last = Math.min(pointCount - 1, Math.max(first, (srcEnd - 1e-9) | 0));

    let sumAmp = 0,
      sumBass = 0,
      sumMid = 0,
      sumHigh = 0,
      count = 0;
    for (let point = first; point <= last; point++) {
      const at = (fromPoint + point) * 4;
      const amp = peaks[at + 3];
      sumAmp += amp;
      sumBass += peaks[at] * amp;
      sumMid += peaks[at + 1] * amp;
      sumHigh += peaks[at + 2] * amp;
      count++;
    }

    const lift = sumAmp > 0 ? 1 / (sumAmp * bandReference) : 0;
    columns.push({
      bass: sumBass * lift,
      mid: sumMid * lift,
      high: sumHigh * lift,
      amp: count > 0 ? sumAmp / count : 0
    });
  }
  return columns;
}

function barHalfHeight(amp: number, height: number, paint: WaveformPaint): number {
  if (amp < SILENT_AMP) return 0;
  const reach = Math.min(Math.sqrt(amp) * paint.ampScale, paint.maxBarFraction);
  return (reach * (height / 2)) | 0;
}

// Each band from its own level, never a share of the amplitude bar: sharing pinned the
// loudest band to a bar that clamps, so bass painted a flat rectangle behind everything.
export function bandHalfHeights(
  column: WaveformColumn,
  height: number,
  paint: WaveformPaint
): [number, number, number] {
  if (column.amp < SILENT_AMP) return [0, 0, 0];
  const half = height / 2;
  const reach = (level: number) =>
    level <= 0
      ? 0
      : (Math.min(Math.sqrt(level) * paint.bandScale, paint.maxBarFraction) * half) | 0;
  return [reach(column.bass), reach(column.mid), reach(column.high)];
}

// Bit per band, so the seven entries are indexed by which bands reach the row.
const STACKED_BY_MASK = [
  'bass',
  'mid',
  'bassMid',
  'high',
  'bassHigh',
  'midHigh',
  'all'
] as const satisfies readonly (keyof StackedPalette)[];

export function stackedRowColor(
  colors: StackedPalette,
  halves: [number, number, number],
  distanceFromCentre: number
): Rgb | null {
  const mask =
    (distanceFromCentre < halves[0] ? 1 : 0) |
    (distanceFromCentre < halves[1] ? 2 : 0) |
    (distanceFromCentre < halves[2] ? 4 : 0);
  return mask === 0 ? null : colors[STACKED_BY_MASK[mask - 1]];
}

// The tallest bar actually drawn, not the amplitude=1 max no real track reaches
// after averaging and the sqrt curve, so a playhead line touches the waveform.
export function maxBarTop(columns: WaveformColumn[], height: number, paint: WaveformPaint): number {
  let tallest = 0;
  for (const column of columns) {
    const half = barHalfHeight(column.amp, height, paint);
    if (half > tallest) tallest = half;
  }
  return Math.max(0, ((height / 2) | 0) - tallest);
}

export function paintWaveformColumns(
  image: ImageData,
  columns: WaveformColumn[],
  paint: WaveformPaint,
  fromColumn = 0,
  toColumn = columns.length
): void {
  const px = image.data;
  const width = image.width;
  const height = image.height;
  const centerRow = (height / 2) | 0;
  const stacked = paint.palette.kind === 'stacked' ? paint.palette.colors : null;
  const balance = paint.palette.kind === 'blended' ? paint.palette.balance : null;

  for (let column = fromColumn; column < toColumn; column++) {
    if (paint.background) {
      const [r, g, b] = paint.background;
      for (let row = 0; row < height; row++) {
        const at = (row * width + column) * 4;
        px[at] = r;
        px[at + 1] = g;
        px[at + 2] = b;
        px[at + 3] = 255;
      }
    }

    const entry = columns[column];
    const halves = stacked ? bandHalfHeights(entry, height, paint) : null;
    const half = halves ? Math.max(...halves) : barHalfHeight(entry.amp, height, paint);
    if (half === 0) {
      if (paint.baseline) {
        const at = (centerRow * width + column) * 4;
        px[at] = paint.baseline[0];
        px[at + 1] = paint.baseline[1];
        px[at + 2] = paint.baseline[2];
        px[at + 3] = 255;
      }
      continue;
    }

    const blended = balance
      ? spectralColor(
          entry.bass * balance[0],
          entry.mid * balance[1],
          entry.high * balance[2],
          entry.amp
        )
      : null;

    const top = Math.max(0, centerRow - half);
    const bottom = Math.min(height, centerRow + half);
    for (let row = top; row < bottom; row++) {
      const rgb =
        stacked && halves ? stackedRowColor(stacked, halves, Math.abs(row - centerRow)) : blended;
      if (!rgb) continue;
      const at = (row * width + column) * 4;
      px[at] = rgb[0];
      px[at + 1] = rgb[1];
      px[at + 2] = rgb[2];
      px[at + 3] = 255;
    }
  }
}

export function waveformImageData(
  width: number,
  height: number,
  columns: WaveformColumn[],
  paint: WaveformPaint
): ImageData {
  const image = new ImageData(width, height);
  paintWaveformColumns(image, columns, paint);
  return image;
}
