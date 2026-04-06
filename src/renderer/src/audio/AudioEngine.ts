/**
 * AudioEngine — loads tracks as AudioBuffers and plays them.
 * Supports an optional loop region that can be activated/deactivated.
 *
 * BPM is set directly (from detection or user input) and drives:
 *   - playback rate: targetBpm / trackBpm
 *   - phase: ((trackPosition - beatOffset) * trackBpm / 60) % 1
 *
 * If trackBpm is null (not yet set), playback runs at original speed and phase is 0.
 */

export type LoopRegion = {
  startSec: number;
  endSec: number;
  beats: number;
};

function advanceBuffer(pos: number, elapsed: number, loopStart: number, loopEnd: number): number {
  const loopDur = loopEnd - loopStart;
  const distToFirstWrap = loopEnd - pos;
  if (elapsed < distToFirstWrap) return pos + elapsed;
  return loopStart + ((elapsed - distToFirstWrap) % loopDur);
}

const EQ_LOW_FREQ = 200;
const EQ_MID_FREQ = 1000;
const EQ_MID_Q = 1;
const EQ_HIGH_FREQ = 8000;
const EQ_GAIN_LIMIT = 12;

export class AudioEngine {
  private ctx: AudioContext;
  private buffer: AudioBuffer | null = null;
  private source: AudioBufferSourceNode | null = null;
  private gainNode: GainNode;
  private eqLow: BiquadFilterNode;
  private eqMid: BiquadFilterNode;
  private eqHigh: BiquadFilterNode;

  private currentRegion: LoopRegion | null = null;
  private loopEnabled = false;
  private bpm: number | null = null;
  private gridOffset = 0;
  private targetRate: number | null = null;
  private isPlaying = false;
  private nudgePercent = 0;

  // bufferAnchorPos: track position at the start of the current playback segment.
  // Persists across stop/start so the track resumes from where it was paused.
  private bufferAnchorPos = 0;
  private lastSegmentStart = 0;

  constructor(ctx: AudioContext) {
    this.ctx = ctx;

    this.eqLow = ctx.createBiquadFilter();
    this.eqLow.type = 'lowshelf';
    this.eqLow.frequency.value = EQ_LOW_FREQ;

    this.eqMid = ctx.createBiquadFilter();
    this.eqMid.type = 'peaking';
    this.eqMid.frequency.value = EQ_MID_FREQ;
    this.eqMid.Q.value = EQ_MID_Q;

    this.eqHigh = ctx.createBiquadFilter();
    this.eqHigh.type = 'highshelf';
    this.eqHigh.frequency.value = EQ_HIGH_FREQ;

    this.gainNode = ctx.createGain();

    this.eqLow.connect(this.eqMid);
    this.eqMid.connect(this.eqHigh);
    this.eqHigh.connect(this.gainNode);
    this.gainNode.connect(ctx.destination);
  }

  setEq(band: 'low' | 'mid' | 'high', db: number): void {
    const node = band === 'low' ? this.eqLow : band === 'mid' ? this.eqMid : this.eqHigh;
    node.gain.value = Math.max(-EQ_GAIN_LIMIT, Math.min(EQ_GAIN_LIMIT, db));
  }

  get playing(): boolean {
    return this.isPlaying;
  }

  get bufferData(): AudioBuffer | null {
    return this.buffer;
  }

  get trackBpm(): number | null {
    return this.bpm;
  }

  set trackBpm(value: number | null) {
    this.bpm = value;
    if (this.isPlaying) this.applyPlaybackRate();
  }

  get beatOffset(): number {
    return this.gridOffset;
  }

  set beatOffset(value: number) {
    this.gridOffset = value;
  }

  get loopActive(): boolean {
    return this.loopEnabled;
  }

  set loopActive(value: boolean) {
    if (this.loopEnabled === value) return;
    if (this.isPlaying) {
      const pos = this.currentBufferPos;
      this.stopSource();
      this.loopEnabled = value;
      this.resumeFrom(pos);
    } else {
      this.loopEnabled = value;
    }
  }

  get targetBpm(): number | null {
    return this.targetRate;
  }

  set targetBpm(value: number | null) {
    if (this.isPlaying) this.flushSegment();
    this.targetRate = value;
    if (this.isPlaying) this.applyPlaybackRate();
  }

  private get effectiveRate(): number {
    if (this.targetRate === null || this.bpm === null) return 1;
    return (this.targetRate * (1 + this.nudgePercent / 100)) / this.bpm;
  }

  get phase(): number {
    if (!this.isPlaying || this.bpm === null) return 0;
    const pos = this.currentBufferPos;
    const beats = ((pos - this.gridOffset) * this.bpm) / 60;
    return ((beats % 1) + 1) % 1;
  }

  get trackPosition(): number | null {
    if (!this.isPlaying) return null;
    return this.currentBufferPos;
  }

  get position(): number {
    return this.isPlaying ? this.currentBufferPos : this.bufferAnchorPos;
  }

  private get currentBufferPos(): number {
    const elapsed = (this.ctx.currentTime - this.lastSegmentStart) * this.effectiveRate;
    if (this.loopEnabled && this.currentRegion) {
      return advanceBuffer(
        this.bufferAnchorPos,
        elapsed,
        this.currentRegion.startSec,
        this.currentRegion.endSec
      );
    }
    const pos = this.bufferAnchorPos + elapsed;
    return this.buffer ? Math.min(pos, this.buffer.duration) : pos;
  }

  // Snapshots elapsed progress into bufferAnchorPos before any rate change
  // so trackPosition and phase stay consistent across the rate change.
  private flushSegment(): void {
    this.bufferAnchorPos = this.currentBufferPos;
    this.lastSegmentStart = this.ctx.currentTime;
  }

  async loadFile(file: File): Promise<void> {
    const arrayBuffer = await file.arrayBuffer();
    this.buffer = await this.ctx.decodeAudioData(arrayBuffer);
    this.currentRegion = null;
    this.loopEnabled = false;
    this.bufferAnchorPos = 0;
  }

  setRegion(region: LoopRegion): void {
    this.currentRegion = region;
    if (this.isPlaying && this.loopEnabled && this.source) {
      this.source.loopStart = region.startSec;
      this.source.loopEnd = region.endSec;
    }
  }

  setNudge(offsetPercent: number): void {
    if (this.isPlaying) this.flushSegment();
    this.nudgePercent = offsetPercent;
    if (this.isPlaying) this.applyPlaybackRate();
  }

  private applyPlaybackRate(): void {
    if (this.source) {
      this.source.playbackRate.value = this.effectiveRate;
    }
  }

  private createSource(fromSec: number, when: number): void {
    if (!this.buffer) return;

    this.bufferAnchorPos = fromSec;
    this.lastSegmentStart = when;

    const src = this.ctx.createBufferSource();
    src.buffer = this.buffer;
    src.playbackRate.value = this.effectiveRate;

    if (this.loopEnabled && this.currentRegion) {
      const { startSec, endSec } = this.currentRegion;
      src.loop = true;
      src.loopStart = startSec;
      src.loopEnd = endSec;
      const effectiveStart = fromSec >= startSec && fromSec < endSec ? fromSec : startSec;
      src.start(when, effectiveStart);
      this.bufferAnchorPos = effectiveStart;
    } else {
      src.loop = false;
      src.start(when, fromSec);
      src.onended = () => {
        if (this.source === src) {
          this.bufferAnchorPos = this.buffer?.duration ?? this.bufferAnchorPos;
          this.source = null;
          this.isPlaying = false;
        }
      };
    }

    src.connect(this.eqLow);
    this.source = src;
    this.isPlaying = true;
  }

  private stopSource(): void {
    try {
      this.source?.stop();
    } catch {
      // ignore errors from already-stopped sources
    }
    this.source?.disconnect();
    this.source = null;
    this.isPlaying = false;
  }

  private resumeFrom(pos: number): void {
    this.createSource(pos, this.ctx.currentTime);
  }

  startAt(when: number, fromSec?: number): void {
    if (!this.buffer || this.isPlaying) return;
    this.createSource(fromSec ?? this.bufferAnchorPos, when);
  }

  stop(): void {
    if (!this.isPlaying) return;
    this.flushSegment();
    this.stopSource();
  }

  seekTo(sec: number): void {
    if (this.isPlaying || !this.buffer) return;
    this.bufferAnchorPos = Math.max(0, Math.min(sec, this.buffer.duration));
  }

  reloop(): void {
    if (!this.currentRegion) return;
    const startSec = this.currentRegion.startSec;
    if (this.isPlaying) {
      this.stopSource();
      this.loopEnabled = true;
      this.resumeFrom(startSec);
    } else {
      this.seekTo(startSec);
    }
  }

  destroy(): void {
    this.stop();
    this.gainNode.disconnect();
  }
}
