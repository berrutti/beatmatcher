// Rendering approach inspired by Mixxx (https://github.com/mixxxdj/mixxx):
// mean-in-window pixel aggregation for stable, LOD-aware waveform display.

const BACKGROUND_R = 10;
const BACKGROUND_G = 10;
const BACKGROUND_B = 10;

export function buildWaveformImageData(
  cw: number,
  ch: number,
  peaks: Float32Array,
  ampScale: number
): ImageData {
  const img = new ImageData(cw, ch);
  const px = img.data;
  for (let i = 0; i < px.length; i += 4) {
    px[i] = BACKGROUND_R; px[i + 1] = BACKGROUND_G; px[i + 2] = BACKGROUND_B; px[i + 3] = 255;
  }
  const halfCh = ch / 2;
  const numPoints = (peaks.length / 4) | 0;
  for (let col = 0; col < cw; col++) {
    const srcStart = (col * numPoints) / cw;
    const srcEnd = ((col + 1) * numPoints) / cw;
    const iStart = srcStart | 0;
    const iEnd = Math.min(numPoints - 1, Math.max(iStart, (srcEnd - 1e-9) | 0));

    let sumAmp = 0, sumR = 0, sumG = 0, sumB = 0, count = 0;
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

    // sqrt maps raw RMS (0–1) to display height with a curve that keeps quiet
    // sections visible and shows real dynamic variation across the track.
    const displayAmp = Math.sqrt(avgAmp);
    const r = sumAmp > 0 ? (sumR / sumAmp * 255) | 0 : 0;
    const g = sumAmp > 0 ? (sumG / sumAmp * 255) | 0 : 0;
    const b = sumAmp > 0 ? (sumB / sumAmp * 255) | 0 : 0;

    const barPx = (displayAmp * halfCh * ampScale) | 0;
    const yTop = Math.max(0, (halfCh | 0) - barPx);
    const yBot = Math.min(ch, (halfCh | 0) + barPx);
    for (let row = yTop; row < yBot; row++) {
      const idx = (row * cw + col) * 4;
      px[idx] = r; px[idx + 1] = g; px[idx + 2] = b;
    }
  }
  return img;
}
