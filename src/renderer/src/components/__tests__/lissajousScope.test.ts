import { describe, it, expect } from 'vitest'
import { computeDotPosition, segmentAlpha } from '../lissajousScope'

describe('computeDotPosition', () => {
  const HALF = 100
  const AMPLITUDE = 82 // 100 * 0.82

  it('returns center when all phases are 0', () => {
    const [x, y] = computeDotPosition([0, 0], AMPLITUDE, HALF)
    expect(x).toBeCloseTo(HALF)
    expect(y).toBeCloseTo(HALF)
  })

  it('returns center when all phases are 0.5 (sin = 0)', () => {
    const [x, y] = computeDotPosition([0.5, 0.5], AMPLITUDE, HALF)
    expect(x).toBeCloseTo(HALF)
    expect(y).toBeCloseTo(HALF)
  })

  it('with 2 sources at phase 0.25: first source (angle 0) pushes x, second (angle π/2) pushes y', () => {
    // phase 0.25 → sin(0.25 * 2π) = sin(π/2) = 1
    // source 0: dirAngle = 0 → displacement * (cos0, sin0) = (1, 0) → x+amplitude
    // source 1: dirAngle = π/2 → displacement * (cos(π/2), sin(π/2)) = (0, 1) → y+amplitude
    const [x, y] = computeDotPosition([0.25, 0.25], AMPLITUDE, HALF)
    expect(x).toBeCloseTo(HALF + AMPLITUDE)
    expect(y).toBeCloseTo(HALF + AMPLITUDE)
  })

  it('with 2 sources at opposite phases: contributions cancel on Y, add on X', () => {
    // source 0 at phase 0.25 (sin=1), source 1 at phase 0.75 (sin=-1)
    // source 0: dirAngle=0 → (+1, 0)
    // source 1: dirAngle=π/2 → (-1·0, -1·1) = (0, -1)
    const [x, y] = computeDotPosition([0.25, 0.75], AMPLITUDE, HALF)
    expect(x).toBeCloseTo(HALF + AMPLITUDE) // only source 0 contributes to x
    expect(y).toBeCloseTo(HALF - AMPLITUDE) // only source 1 contributes to y (negated)
  })

  it('with 1 source at phase 0.25: dot moves along the single direction axis', () => {
    // n=1 → dirAngle = 0 → pushes purely in x
    const [x, y] = computeDotPosition([0.25], AMPLITUDE, HALF)
    expect(x).toBeCloseTo(HALF + AMPLITUDE)
    expect(y).toBeCloseTo(HALF)
  })

  it('output is symmetric: negating all phases negates the displacement from center', () => {
    const phases = [0.1, 0.3]
    const [x1, y1] = computeDotPosition(phases, AMPLITUDE, HALF)
    // negating phase p: sin(-p * 2π) = -sin(p * 2π), so displacement flips
    const negated = phases.map(p => -p)
    const [x2, y2] = computeDotPosition(negated, AMPLITUDE, HALF)
    expect(x2).toBeCloseTo(2 * HALF - x1)
    expect(y2).toBeCloseTo(2 * HALF - y1)
  })
})

describe('segmentAlpha', () => {
  it('returns 0 for the first segment (i=1 in a long history)', () => {
    // t = 1/99 ≈ 0.0101, t² ≈ 0.000102
    const alpha = segmentAlpha(1, 100, 1)
    expect(alpha).toBeCloseTo((1 / 99) ** 2)
  })

  it('returns 1 for the last segment at full fadeFactor', () => {
    // t = (n-1)/(n-1) = 1, t² = 1, * fadeFactor 1 = 1
    const alpha = segmentAlpha(9, 10, 1)
    expect(alpha).toBeCloseTo(1)
  })

  it('scales linearly with fadeFactor', () => {
    const full = segmentAlpha(5, 10, 1)
    const half = segmentAlpha(5, 10, 0.5)
    expect(half).toBeCloseTo(full * 0.5)
  })

  it('returns 0 when fadeFactor is 0', () => {
    expect(segmentAlpha(5, 10, 0)).toBe(0)
  })

  it('is quadratic in t: middle segment is 0.25 of the last', () => {
    // t for i=5 in historyLength=11 is 0.5, t²=0.25
    // t for i=10 in historyLength=11 is 1, t²=1
    const mid = segmentAlpha(5, 11, 1)
    const last = segmentAlpha(10, 11, 1)
    expect(mid).toBeCloseTo(last * 0.25)
  })
})
