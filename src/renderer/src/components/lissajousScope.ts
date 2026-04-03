export function computeDotPosition(
  phases: number[],
  amplitude: number,
  half: number
): [number, number] {
  const n = phases.length
  let sumX = 0
  let sumY = 0
  for (let i = 0; i < n; i++) {
    // Semicircle (π) instead of full circle so opposite sources don't cancel each other out
    const dirAngle = (i * Math.PI) / n
    // Phase is [0,1]; map to [-1,1] displacement via sin so the dot oscillates back and forth
    const displacement = Math.sin(phases[i] * Math.PI * 2)
    sumX += displacement * Math.cos(dirAngle)
    sumY += displacement * Math.sin(dirAngle)
  }
  return [half + sumX * amplitude, half + sumY * amplitude]
}

export function segmentAlpha(i: number, historyLength: number, fadeFactor: number): number {
  const t = i / (historyLength - 1)
  // Quadratic so the tail fades quickly while the head stays bright
  return t * t * fadeFactor
}
