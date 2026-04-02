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
  startSec: number
  endSec: number
  beats: 16 | 32
}

export class LoopEngine {
  private ctx: AudioContext
  private buffer: AudioBuffer | null = null
  private source: AudioBufferSourceNode | null = null
  private gainNode: GainNode
  private eqLow: BiquadFilterNode
  private eqMid: BiquadFilterNode
  private eqHigh: BiquadFilterNode

  private _region: LoopRegion | null = null
  private _targetBpm = 138
  private _playing = false
  private _nudgePercent = 0

  private accumulatedPhase = 0
  private lastSegmentStart = 0

  constructor(ctx: AudioContext) {
    this.ctx = ctx

    this.eqLow = ctx.createBiquadFilter()
    this.eqLow.type = 'lowshelf'
    this.eqLow.frequency.value = 200

    this.eqMid = ctx.createBiquadFilter()
    this.eqMid.type = 'peaking'
    this.eqMid.frequency.value = 1000
    this.eqMid.Q.value = 1

    this.eqHigh = ctx.createBiquadFilter()
    this.eqHigh.type = 'highshelf'
    this.eqHigh.frequency.value = 8000

    this.gainNode = ctx.createGain()

    this.eqLow.connect(this.eqMid)
    this.eqMid.connect(this.eqHigh)
    this.eqHigh.connect(this.gainNode)
    this.gainNode.connect(ctx.destination)
  }

  setEq(band: 'low' | 'mid' | 'high', db: number): void {
    const node = band === 'low' ? this.eqLow : band === 'mid' ? this.eqMid : this.eqHigh
    node.gain.value = Math.max(-12, Math.min(12, db))
  }

  get audioContext(): AudioContext {
    return this.ctx
  }

  get playing(): boolean {
    return this._playing
  }

  get region(): LoopRegion | null {
    return this._region
  }

  get buffer_(): AudioBuffer | null {
    return this.buffer
  }

  /** BPM inferred from the loop region + beat count */
  get inferredBpm(): number {
    if (!this._region) return this._targetBpm
    const dur = this._region.endSec - this._region.startSec
    if (dur <= 0) return this._targetBpm
    return (this._region.beats * 60) / dur
  }

  get targetBpm(): number {
    return this._targetBpm
  }

  set targetBpm(value: number) {
    this._targetBpm = value
    if (this._playing) this.applyPlaybackRate()
  }

  private get effectiveTargetBpm(): number {
    return this._targetBpm * (1 + this._nudgePercent / 100)
  }

  private get playbackRate(): number {
    const inferred = this.inferredBpm
    if (inferred <= 0) return 1
    return this.effectiveTargetBpm / inferred
  }

  async loadFile(file: File): Promise<void> {
    const arrayBuffer = await file.arrayBuffer()
    this.buffer = await this.ctx.decodeAudioData(arrayBuffer)
    // Reset region when new file is loaded
    this._region = null
  }

  setRegion(region: LoopRegion): void {
    this._region = region
    if (this._playing && this.source) {
      // Update loop boundaries on the live source — no restart needed
      this.source.loopStart = region.startSec
      this.source.loopEnd = region.endSec
      this.applyPlaybackRate()
    }
  }

  setBeats(beats: 16 | 32): void {
    if (!this._region) return
    // Non-destructive: keep start, scale end so duration doubles/halves
    const oldBeats = this._region.beats
    const oldDur = this._region.endSec - this._region.startSec
    const newDur = oldDur * (beats / oldBeats)
    this._region = {
      ...this._region,
      beats,
      endSec: this._region.startSec + newDur
    }
    if (this._playing) {
      this.stop()
      this.start()
    }
  }

  setNudge(offsetPercent: number): void {
    if (this._playing) this.flushSegment()
    this._nudgePercent = offsetPercent
    if (this._playing) this.applyPlaybackRate()
  }

  private applyPlaybackRate(): void {
    if (this.source) {
      this.source.playbackRate.value = this.playbackRate
    }
  }

  private flushSegment(): void {
    const now = this.ctx.currentTime
    this.accumulatedPhase += (now - this.lastSegmentStart) * this.effectiveTargetBpm / 60
    this.lastSegmentStart = now
  }

  getPhase(): number {
    if (!this._playing) return 0
    const now = this.ctx.currentTime
    const total = this.accumulatedPhase + (now - this.lastSegmentStart) * this.effectiveTargetBpm / 60
    return total % 1.0
  }

  /** Position within the loop region in seconds (wraps around on each loop). */
  getLoopPositionSec(): number {
    if (!this._playing || !this._region) return this._region?.startSec ?? 0
    const dur = this._region.endSec - this._region.startSec
    if (dur <= 0) return this._region.startSec
    const now = this.ctx.currentTime
    // Total beats elapsed, then convert to seconds within loop at playback rate
    const totalBeats = this.accumulatedPhase + (now - this.lastSegmentStart) * this.effectiveTargetBpm / 60
    const loopBeats = this._region.beats
    const positionInLoop = (totalBeats % loopBeats) / loopBeats
    return this._region.startSec + positionInLoop * dur
  }

  start(): void {
    this.startAt(this.ctx.currentTime)
  }

  startAt(when: number): void {
    if (!this.buffer || !this._region || this._playing) return

    const { startSec, endSec } = this._region
    const dur = endSec - startSec
    if (dur <= 0) return

    this.accumulatedPhase = 0
    this.lastSegmentStart = when

    const src = this.ctx.createBufferSource()
    src.buffer = this.buffer
    src.loop = true
    src.loopStart = startSec
    src.loopEnd = endSec
    src.playbackRate.value = this.playbackRate
    src.connect(this.eqLow)
    src.start(when, startSec)

    this.source = src
    this._playing = true
  }

  stop(): void {
    if (!this._playing) return
    try { this.source?.stop() } catch {}
    this.source?.disconnect()
    this.source = null
    this._playing = false
  }

  cue(): void {
    this.stop()
    this.accumulatedPhase = 0
    this.lastSegmentStart = 0
  }

  destroy(): void {
    this.stop()
    this.gainNode.disconnect()
  }
}
