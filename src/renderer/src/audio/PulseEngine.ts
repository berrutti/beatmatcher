/**
 * PulseEngine — Web Audio API look-ahead scheduler.
 *
 * Uses the classic "dual clock" approach:
 *  - A JS setInterval ticks every ~25ms to schedule upcoming beats
 *  - Each beat is scheduled directly on the AudioContext timeline,
 *    so timing is sample-accurate regardless of JS jitter.
 *
 * Phase tracking uses an accumulated-phase model so nudging
 * (temporary BPM offset) is correctly reflected in getPhase().
 */

const LOOKAHEAD_MS = 100.0
const SCHEDULE_INTERVAL = 25

export type PulseEngineOptions = {
  bpm: number
}

export class PulseEngine {
  private ctx: AudioContext
  private intervalId: ReturnType<typeof setInterval> | null = null
  private nextBeatTime = 0
  private _bpm: number
  private _playing = false
  private nudgeBpmOffset = 0

  // Phase tracking
  private accumulatedPhase = 0
  private lastSegmentStart = 0

  constructor(options: PulseEngineOptions) {
    this.ctx = new AudioContext()
    this._bpm = options.bpm
  }

  get audioContext(): AudioContext {
    return this.ctx
  }

  get bpm(): number {
    return this._bpm
  }

  set bpm(value: number) {
    this._bpm = Math.max(20, Math.min(300, value))
    // Flush segment so phase stays continuous across BPM slider moves
    if (this._playing) this.flushSegment()
  }

  get playing(): boolean {
    return this._playing
  }

  private get effectiveBpm(): number {
    return this._bpm * (1 + this.nudgeBpmOffset / 100)
  }

  private get secondsPerBeat(): number {
    return 60 / this.effectiveBpm
  }

  /**
   * Flush current phase segment into accumulatedPhase.
   * Must be called BEFORE changing effectiveBpm.
   */
  private flushSegment(): void {
    const now = this.ctx.currentTime
    this.accumulatedPhase += (now - this.lastSegmentStart) * this.effectiveBpm / 60
    this.lastSegmentStart = now
  }

  /**
   * Returns phase within the current beat: 0.0 (beat start) → 1.0 (next beat).
   * Returns 0 when not playing.
   */
  getPhase(): number {
    if (!this._playing) return 0
    const now = this.ctx.currentTime
    const total = this.accumulatedPhase + (now - this.lastSegmentStart) * this.effectiveBpm / 60
    return total % 1.0
  }

  /**
   * Temporarily offset BPM for nudging (pitch bend).
   * Pass 0 to release the nudge.
   */
  setNudge(offsetPercent: number): void {
    // Flush segment BEFORE changing the offset
    if (this._playing) this.flushSegment()
    this.nudgeBpmOffset = offsetPercent
  }

  start(): void {
    if (this._playing) return

    if (this.ctx.state === 'suspended') {
      this.ctx.resume()
    }

    // Reset phase
    this.accumulatedPhase = 0
    this.lastSegmentStart = this.ctx.currentTime

    this._playing = true
    this.nextBeatTime = this.ctx.currentTime
    this.intervalId = setInterval(() => this.schedule(), SCHEDULE_INTERVAL)
  }

  stop(): void {
    if (!this._playing) return
    this._playing = false
    if (this.intervalId !== null) {
      clearInterval(this.intervalId)
      this.intervalId = null
    }
  }

  cue(): void {
    this.stop()
    this.accumulatedPhase = 0
    this.lastSegmentStart = 0
  }

  private schedule(): void {
    const lookAheadUntil = this.ctx.currentTime + LOOKAHEAD_MS / 1000

    while (this.nextBeatTime < lookAheadUntil) {
      this.scheduleClick(this.nextBeatTime)
      this.nextBeatTime += this.secondsPerBeat
    }
  }

  private scheduleClick(time: number): void {
    const osc = this.ctx.createOscillator()
    const gain = this.ctx.createGain()

    osc.connect(gain)
    gain.connect(this.ctx.destination)

    osc.frequency.setValueAtTime(1000, time)
    gain.gain.setValueAtTime(0.0, time)
    gain.gain.linearRampToValueAtTime(0.8, time + 0.002)
    gain.gain.exponentialRampToValueAtTime(0.0001, time + 0.05)

    osc.start(time)
    osc.stop(time + 0.06)

    osc.onended = () => {
      osc.disconnect()
      gain.disconnect()
    }
  }

  destroy(): void {
    this.stop()
    this.ctx.close()
  }
}
