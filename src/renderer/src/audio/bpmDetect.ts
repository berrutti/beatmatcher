export interface BpmCandidate {
  tempo: number
  count: number
}

export interface BpmResult {
  bpm: number
  confidence: number
  candidates: BpmCandidate[]
}

export const BPM_MIN = 90
export const BPM_MAX = 180
export const PEAK_SKIP_SAMPLES = 10000
export const NEIGHBOR_COUNT = 10
export const CLUSTER_TOLERANCE = 1.0
export const THRESHOLDS = [0.9, 0.8, 0.7]
export const MIN_PEAKS = 15

export function getPeaks(data: Float32Array, threshold: number, skipSamples = PEAK_SKIP_SAMPLES): number[] {
  const peaks: number[] = []
  let i = 0
  while (i < data.length) {
    if (Math.abs(data[i]) > threshold) {
      peaks.push(i)
      i += skipSamples
    }
    i++
  }
  return peaks
}

export function countIntervals(peaks: number[], neighbors = NEIGHBOR_COUNT): Map<number, number> {
  const counts = new Map<number, number>()
  for (let idx = 0; idx < peaks.length; idx++) {
    const limit = Math.min(idx + neighbors, peaks.length - 1)
    for (let j = idx + 1; j <= limit; j++) {
      const interval = peaks[j] - peaks[idx]
      counts.set(interval, (counts.get(interval) ?? 0) + 1)
    }
  }
  return counts
}

export function intervalToBpm(intervalSamples: number, sampleRate: number): number | null {
  if (intervalSamples <= 0) return null
  let bpm = 60 / (intervalSamples / sampleRate)
  let iterations = 0
  while (bpm < BPM_MIN && iterations < 10) { bpm *= 2; iterations++ }
  iterations = 0
  while (bpm > BPM_MAX && iterations < 10) { bpm /= 2; iterations++ }
  if (bpm < BPM_MIN || bpm > BPM_MAX) return null
  return bpm
}

export function groupByTempo(
  intervalCounts: Map<number, number>,
  sampleRate: number
): BpmCandidate[] {
  const clusters: { tempoSum: number; count: number }[] = []

  for (const [interval, count] of intervalCounts) {
    const bpm = intervalToBpm(interval, sampleRate)
    if (bpm === null) continue

    let merged = false
    for (const cluster of clusters) {
      const clusterBpm = cluster.tempoSum / cluster.count
      if (Math.abs(clusterBpm - bpm) <= CLUSTER_TOLERANCE) {
        cluster.tempoSum += bpm * count
        cluster.count += count
        merged = true
        break
      }
    }
    if (!merged) {
      clusters.push({ tempoSum: bpm * count, count })
    }
  }

  return clusters
    .map(c => ({ tempo: c.tempoSum / c.count, count: c.count }))
    .sort((a, b) => b.count - a.count)
}

export function detectBpmFromSamples(data: Float32Array, sampleRate: number): BpmResult {
  let peaks: number[] = []

  for (const threshold of THRESHOLDS) {
    peaks = getPeaks(data, threshold)
    if (peaks.length >= MIN_PEAKS) break
  }

  if (peaks.length < 2) {
    return { bpm: 0, confidence: 0, candidates: [] }
  }

  const intervals = countIntervals(peaks)
  const candidates = groupByTempo(intervals, sampleRate)

  if (candidates.length === 0) {
    return { bpm: 0, confidence: 0, candidates: [] }
  }

  const top = candidates[0]
  const second = candidates[1]
  const confidence = second
    ? Math.min(1, top.count / (top.count + second.count))
    : 1

  return {
    bpm: Math.round(top.tempo * 10) / 10,
    confidence: Math.round(confidence * 100) / 100,
    candidates: candidates.slice(0, 5)
  }
}

async function lowPassFilter(buffer: AudioBuffer): Promise<Float32Array> {
  const ctx = new OfflineAudioContext(1, buffer.length, buffer.sampleRate)
  const source = ctx.createBufferSource()
  source.buffer = buffer

  const filter = ctx.createBiquadFilter()
  filter.type = 'lowpass'
  filter.frequency.value = 150

  source.connect(filter)
  filter.connect(ctx.destination)
  source.start(0)

  const rendered = await ctx.startRendering()
  return rendered.getChannelData(0)
}

export async function detectBpm(buffer: AudioBuffer): Promise<BpmResult> {
  const filtered = await lowPassFilter(buffer)
  return detectBpmFromSamples(filtered, buffer.sampleRate)
}
