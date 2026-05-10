// Rendering approach inspired by Mixxx (https://github.com/mixxxdj/mixxx):
// mean-in-window pixel aggregation for stable, LOD-aware waveform display.

const BACKGROUND_R = 10;
const BACKGROUND_G = 10;
const BACKGROUND_B = 10;

// Log-compress each spectral band independently so all frequency ranges are visible.
// Bass naturally dominates by energy, so it gets a smaller multiplier; highs get a
// larger one so hi-hats and presence reach a comparable brightness.
const LOG_MUL: [number, number, number] = [10, 25, 75];

export function spectralColor(bass: number, mid: number, high: number): [number, number, number] {
  const r = ((Math.log1p(bass * LOG_MUL[0]) / Math.log1p(LOG_MUL[0])) * 255) | 0;
  const g = ((Math.log1p(mid * LOG_MUL[1]) / Math.log1p(LOG_MUL[1])) * 255) | 0;
  const b = ((Math.log1p(high * LOG_MUL[2]) / Math.log1p(LOG_MUL[2])) * 255) | 0;
  return [r, g, b];
}

export function buildWaveformImageData(
  cw: number,
  ch: number,
  peaks: Float32Array,
  ampScale: number
): ImageData {
  const img = new ImageData(cw, ch);
  const px = img.data;
  for (let i = 0; i < px.length; i += 4) {
    px[i] = BACKGROUND_R;
    px[i + 1] = BACKGROUND_G;
    px[i + 2] = BACKGROUND_B;
    px[i + 3] = 255;
  }
  const halfCh = ch / 2;
  const numPoints = (peaks.length / 4) | 0;
  for (let col = 0; col < cw; col++) {
    const srcStart = (col * numPoints) / cw;
    const srcEnd = ((col + 1) * numPoints) / cw;
    const iStart = srcStart | 0;
    const iEnd = Math.min(numPoints - 1, Math.max(iStart, (srcEnd - 1e-9) | 0));

    let sumAmp = 0,
      sumR = 0,
      sumG = 0,
      sumB = 0,
      count = 0;
    for (let i = iStart; i <= iEnd; i++) {
      const si = i * 4;
      const amp = peaks[si + 3];
      sumAmp += amp;
      sumR += peaks[si] * amp;
      sumG += peaks[si + 1] * amp;
      sumB += peaks[si + 2] * amp;
      count++;
    }

    const avgAmp = count > 0 ? sumAmp / count : 0;
    if (avgAmp < 0.001) continue;

    const displayAmp = Math.sqrt(avgAmp);
    const avgBass = sumAmp > 0 ? sumR / sumAmp : 0;
    const avgMid = sumAmp > 0 ? sumG / sumAmp : 0;
    const avgHigh = sumAmp > 0 ? sumB / sumAmp : 0;
    const [r, g, b] = spectralColor(avgBass, avgMid, avgHigh);

    const barPx = (displayAmp * halfCh * ampScale) | 0;
    const yTop = Math.max(0, (halfCh | 0) - barPx);
    const yBot = Math.min(ch, (halfCh | 0) + barPx);
    for (let row = yTop; row < yBot; row++) {
      const idx = (row * cw + col) * 4;
      px[idx] = r;
      px[idx + 1] = g;
      px[idx + 2] = b;
    }
  }
  return img;
}
