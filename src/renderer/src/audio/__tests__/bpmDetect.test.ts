import { describe, it, expect } from 'vitest';
import {
  getPeaks,
  countIntervals,
  intervalToBpm,
  groupByTempo,
  detectBpmFromSamples
} from '../bpmDetect';

const SAMPLE_RATE = 44100;

function generateClickTrack(
  bpm: number,
  durationSec: number,
  sampleRate = SAMPLE_RATE
): Float32Array {
  const totalSamples = Math.floor(durationSec * sampleRate);
  const data = new Float32Array(totalSamples);
  const samplesPerBeat = (60 / bpm) * sampleRate;
  const clickWidth = 200;

  let pos = 0;
  while (pos < totalSamples) {
    const start = Math.floor(pos);
    for (let i = start; i < Math.min(start + clickWidth, totalSamples); i++) {
      data[i] = 1.0;
    }
    pos += samplesPerBeat;
  }
  return data;
}

function generateNoisyClickTrack(bpm: number, durationSec: number, noiseLevel = 0.3): Float32Array {
  const data = generateClickTrack(bpm, durationSec);
  for (let i = 0; i < data.length; i++) {
    data[i] += (Math.random() - 0.5) * 2 * noiseLevel;
  }
  return data;
}

describe('getPeaks', () => {
  it('finds peaks in a simple click track', () => {
    const data = generateClickTrack(120, 10);
    const peaks = getPeaks(data, 0.9);
    expect(peaks.length).toBeGreaterThanOrEqual(15);
    expect(peaks.length).toBeLessThanOrEqual(25);
  });

  it('returns no peaks for silence', () => {
    const data = new Float32Array(44100 * 5);
    const peaks = getPeaks(data, 0.9);
    expect(peaks.length).toBe(0);
  });

  it('respects threshold - higher threshold means fewer peaks', () => {
    const data = generateNoisyClickTrack(120, 10, 0.5);
    const peaksHigh = getPeaks(data, 0.95);
    const peaksLow = getPeaks(data, 0.7);
    expect(peaksLow.length).toBeGreaterThanOrEqual(peaksHigh.length);
  });
});

describe('intervalToBpm', () => {
  it('converts a known interval to BPM', () => {
    const bpm = intervalToBpm(22050, SAMPLE_RATE);
    expect(bpm).toBeCloseTo(120, 0);
  });

  it('folds a too-slow tempo up by powers of 2', () => {
    const bpm = intervalToBpm(44100, SAMPLE_RATE);
    expect(bpm).toBeCloseTo(120, 0);
  });

  it('folds a too-fast tempo down by powers of 2', () => {
    const bpm = intervalToBpm(11025, SAMPLE_RATE);
    expect(bpm).toBeCloseTo(120, 0);
  });

  it('returns null for zero or negative intervals', () => {
    expect(intervalToBpm(0, SAMPLE_RATE)).toBeNull();
    expect(intervalToBpm(-100, SAMPLE_RATE)).toBeNull();
  });
});

describe('countIntervals', () => {
  it('counts intervals between peaks', () => {
    const peaks = [0, 100, 200, 300];
    const intervals = countIntervals(peaks, 3);
    expect(intervals.get(100)).toBe(3);
    expect(intervals.get(200)).toBe(2);
    expect(intervals.get(300)).toBe(1);
  });

  it('handles single peak', () => {
    const intervals = countIntervals([0], 10);
    expect(intervals.size).toBe(0);
  });
});

describe('groupByTempo', () => {
  it('clusters nearby tempos together', () => {
    const intervals = new Map<number, number>();
    const samplesAt120 = (60 / 120) * SAMPLE_RATE;
    intervals.set(samplesAt120, 10);
    intervals.set(samplesAt120 + 50, 8);

    const candidates = groupByTempo(intervals, SAMPLE_RATE);
    expect(candidates.length).toBe(1);
    expect(candidates[0].tempo).toBeCloseTo(120, 0);
    expect(candidates[0].count).toBe(18);
  });

  it('separates distinct tempos', () => {
    const intervals = new Map<number, number>();
    const samplesAt120 = (60 / 120) * SAMPLE_RATE;
    const samplesAt140 = (60 / 140) * SAMPLE_RATE;
    intervals.set(samplesAt120, 10);
    intervals.set(samplesAt140, 8);

    const candidates = groupByTempo(intervals, SAMPLE_RATE);
    expect(candidates.length).toBe(2);
    expect(candidates[0].tempo).toBeCloseTo(120, 0);
    expect(candidates[1].tempo).toBeCloseTo(140, 0);
  });
});

describe('detectBpmFromSamples', () => {
  it('detects 120 BPM from a clean click track', () => {
    const data = generateClickTrack(120, 30);
    const result = detectBpmFromSamples(data, SAMPLE_RATE);
    expect(result.bpm).toBeCloseTo(120, 0);
    expect(result.confidence).toBeGreaterThan(0.5);
  });

  it('detects 140 BPM from a clean click track', () => {
    const data = generateClickTrack(140, 30);
    const result = detectBpmFromSamples(data, SAMPLE_RATE);
    expect(result.bpm).toBeCloseTo(140, 0);
    expect(result.confidence).toBeGreaterThan(0.5);
  });

  it('detects 128 BPM from a clean click track', () => {
    const data = generateClickTrack(128, 30);
    const result = detectBpmFromSamples(data, SAMPLE_RATE);
    expect(result.bpm).toBeCloseTo(128, 0);
    expect(result.confidence).toBeGreaterThan(0.5);
  });

  it('detects 100 BPM (folded from sub-90 range)', () => {
    const data = generateClickTrack(100, 30);
    const result = detectBpmFromSamples(data, SAMPLE_RATE);
    expect(result.bpm).toBeCloseTo(100, 0);
  });

  it('detects 174 BPM (drum and bass range)', () => {
    const data = generateClickTrack(174, 30);
    const result = detectBpmFromSamples(data, SAMPLE_RATE);
    expect(result.bpm).toBeCloseTo(174, 0);
  });

  it('handles noisy signal at 128 BPM', () => {
    const data = generateNoisyClickTrack(128, 30, 0.3);
    const result = detectBpmFromSamples(data, SAMPLE_RATE);
    expect(Math.abs(result.bpm - 128)).toBeLessThan(2);
  });

  it('returns 0 BPM for silence', () => {
    const data = new Float32Array(SAMPLE_RATE * 10);
    const result = detectBpmFromSamples(data, SAMPLE_RATE);
    expect(result.bpm).toBe(0);
    expect(result.confidence).toBe(0);
  });

  it('returns candidates sorted by count', () => {
    const data = generateClickTrack(130, 30);
    const result = detectBpmFromSamples(data, SAMPLE_RATE);
    for (let i = 1; i < result.candidates.length; i++) {
      expect(result.candidates[i - 1].count).toBeGreaterThanOrEqual(result.candidates[i].count);
    }
  });
});
