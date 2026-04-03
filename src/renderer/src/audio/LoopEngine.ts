/**
 * LoopEngine — loads an AudioBuffer, defines a loop region,
 * and plays it at the correct playbackRate so the loop duration
 * matches the target BPM.
 *
 * BPM is inferred entirely from the loop region:
 *   bpm = (beatCount × 60) / loopDuration
 *
 */

export type LoopRegion = {
  startSec: number;
  endSec: number;
  beats: 16 | 32;
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
const DEFAULT_BPM = 138;

export class LoopEngine {
  private ctx: AudioContext;
  private buffer: AudioBuffer | null = null;
  private source: AudioBufferSourceNode | null = null;
  private gainNode: GainNode;
  private eqLow: BiquadFilterNode;
  private eqMid: BiquadFilterNode;
  private eqHigh: BiquadFilterNode;

  private _region: LoopRegion | null = null;
  private _targetBpm = DEFAULT_BPM;
  private _playing = false;
  private _nudgePercent = 0;

  private accumulatedPhase = 0;
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
    return this._playing;
  }

  get region(): LoopRegion | null {
    return this._region;
  }

  get buffer_(): AudioBuffer | null {
    return this.buffer;
  }

  /** BPM inferred from the loop region + beat count */
  get inferredBpm(): number {
    if (!this._region) return this._targetBpm;
    const dur = this._region.endSec - this._region.startSec;
    if (dur <= 0) return this._targetBpm;
    return (this._region.beats * 60) / dur;
  }

  get targetBpm(): number {
    return this._targetBpm;
  }

  set targetBpm(value: number) {
    this._targetBpm = value;
    if (this._playing) this.applyPlaybackRate();
  }

  private get effectiveTargetBpm(): number {
    return this._targetBpm * (1 + this._nudgePercent / 100);
  }

  private get playbackRate(): number {
    const inferred = this.inferredBpm;
    if (inferred <= 0) return 1;
    return this.effectiveTargetBpm / inferred;
  }

  async loadFile(file: File): Promise<void> {
    const arrayBuffer = await file.arrayBuffer();
    this.buffer = await this.ctx.decodeAudioData(arrayBuffer);
    // Reset region when new file is loaded
    this._region = null;
  }

  setRegion(region: LoopRegion): void {
    if (this._playing && this.source) {
      this.flushSegment();
      this._region = region;
      this.source.loopStart = region.startSec;
      this.source.loopEnd = region.endSec;
      this.applyPlaybackRate();
    } else {
      this._region = region;
    }
  }

  setBeats(beats: 16 | 32): void {
    if (!this._region) return;
    // Non-destructive: keep start, scale end so duration doubles/halves
    const oldBeats = this._region.beats;
    const oldDur = this._region.endSec - this._region.startSec;
    const newDur = oldDur * (beats / oldBeats);
    this._region = {
      ...this._region,
      beats,
      endSec: this._region.startSec + newDur
    };
    if (this._playing) {
      this.stop();
      this.start();
    }
  }

  setNudge(offsetPercent: number): void {
    if (this._playing) this.flushSegment();
    this._nudgePercent = offsetPercent;
    if (this._playing) this.applyPlaybackRate();
  }

  private applyPlaybackRate(): void {
    if (this.source) {
      this.source.playbackRate.value = this.playbackRate;
    }
  }

  private flushSegment(): void {
    const now = this.ctx.currentTime;
    const dt = now - this.lastSegmentStart;
    this.accumulatedPhase += (dt * this.effectiveTargetBpm) / 60;
    if (this._region) {
      this.bufferAnchorPos = advanceBuffer(
        this.bufferAnchorPos,
        dt * this.playbackRate,
        this._region.startSec,
        this._region.endSec
      );
    }
    this.lastSegmentStart = now;
  }

  get phase(): number {
    if (!this._playing) return 0;
    const now = this.ctx.currentTime;
    const total =
      this.accumulatedPhase + ((now - this.lastSegmentStart) * this.effectiveTargetBpm) / 60;
    return total % 1.0;
  }

  /** Absolute position in the track in seconds, follows actual buffer playback through loop boundary changes. Null when not playing. */
  get trackPosition(): number | null {
    if (!this._playing) return null;
    if (!this._region) {
      // this should never happen: _playing is only true when a region is set
      throw new Error('LoopEngine: playing without a region');
    }
    const elapsed = (this.ctx.currentTime - this.lastSegmentStart) * this.playbackRate;
    return advanceBuffer(this.bufferAnchorPos, elapsed, this._region.startSec, this._region.endSec);
  }

  start(): void {
    this.startAt(this.ctx.currentTime);
  }

  startAt(when: number): void {
    if (!this.buffer || !this._region || this._playing) return;

    const { startSec, endSec } = this._region;
    const dur = endSec - startSec;
    if (dur <= 0) return;

    this.accumulatedPhase = 0;
    this.bufferAnchorPos = startSec;
    this.lastSegmentStart = when;

    const src = this.ctx.createBufferSource();
    src.buffer = this.buffer;
    src.loop = true;
    src.loopStart = startSec;
    src.loopEnd = endSec;
    src.playbackRate.value = this.playbackRate;
    src.connect(this.eqLow);
    src.start(when, startSec);

    this.source = src;
    this._playing = true;
  }

  stop(): void {
    if (!this._playing) return;
    try {
      this.source?.stop();
    } catch {
      // fine to ignore these erros
    }
    this.source?.disconnect();
    this.source = null;
    this._playing = false;
  }

  cue(): void {
    this.stop();
    this.accumulatedPhase = 0;
    this.lastSegmentStart = 0;
  }

  destroy(): void {
    this.stop();
    this.gainNode.disconnect();
  }
}
