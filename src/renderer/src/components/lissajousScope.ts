export function computeDotPosition(
  phases: number[],
  amplitude: number,
  cx: number,
  cy: number
): [number, number] {
  let sumX = 0;
  let sumY = 0;
  for (let i = 0; i < phases.length; i++) {
    const angle = (i * Math.PI) / 2;
    const displacement = Math.sin(phases[i] * Math.PI * 2);
    sumX += displacement * Math.cos(angle);
    sumY += displacement * Math.sin(angle);
  }
  const mag = Math.hypot(sumX, sumY);
  if (mag > 1) {
    sumX /= mag;
    sumY /= mag;
  }
  return [cx + sumX * amplitude, cy + sumY * amplitude];
}

export function segmentAlpha(i: number, historyLength: number, fadeFactor: number): number {
  const t = i / (historyLength - 1);
  return t * t * fadeFactor;
}
