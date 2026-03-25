/**
 * PulseEngine — Web Audio API beat scheduler.
 *
 * Schedules one beat at a time: after firing each beat, a setTimeout fires
 * ~10ms before the next beat is due to schedule it. This means nudge changes
 * take effect on the very next beat with no lookahead bleed.
 */

const SCHEDULE_AHEAD_MS = 10

export type PulseEngineOptions = {
  bpm: number
  frequency?: number
}

export class PulseEngine {
  private ctx: AudioContext
  private timeoutId: ReturnType<typeof setTimeout> | null = null
  private nextBeatTime = 0
  private _bpm: number
  private _frequency: number
  private _playing = false
  private nudgeBpmOffset = 0

  // Phase tracking
  private accumulatedPhase = 0
  private lastSegmentStart = 0

  constructor(options: PulseEngineOptions) {
    this.ctx = new AudioContext()
    this._bpm = options.bpm
    this._frequency = options.frequency ?? 1000
  }

  get audioContext(): AudioContext {
    return this.ctx
  }

  get bpm(): number {
    return this._bpm
  }

  set bpm(value: number) {
    this._bpm = Math.max(20, Math.min(300, value))
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

  private flushSegment(): void {
    const now = this.ctx.currentTime
    this.accumulatedPhase += (now - this.lastSegmentStart) * this.effectiveBpm / 60
    this.lastSegmentStart = now
  }

  getPhase(): number {
    if (!this._playing) return 0
    const now = this.ctx.currentTime
    const total = this.accumulatedPhase + (now - this.lastSegmentStart) * this.effectiveBpm / 60
    return total % 1.0
  }

  setNudge(offsetPercent: number): void {
    if (this._playing) this.flushSegment()
    this.nudgeBpmOffset = offsetPercent
  }

  start(): void {
    this.startAt(this.ctx.currentTime)
  }

  startAt(when: number): void {
    if (this._playing) return
    if (this.ctx.state === 'suspended') this.ctx.resume()

    this.accumulatedPhase = 0
    this.lastSegmentStart = when
    this._playing = true
    this.nextBeatTime = when
    // Delay scheduleNext until just before `when`
    const msUntil = (when - this.ctx.currentTime) * 1000
    this.timeoutId = setTimeout(() => this.scheduleNext(), Math.max(0, msUntil - SCHEDULE_AHEAD_MS))
  }

  stop(): void {
    if (!this._playing) return
    this._playing = false
    if (this.timeoutId !== null) {
      clearTimeout(this.timeoutId)
      this.timeoutId = null
    }
  }

  cue(): void {
    this.stop()
    this.accumulatedPhase = 0
    this.lastSegmentStart = 0
  }

  private scheduleNext(): void {
    if (!this._playing) return

    // Schedule the click for nextBeatTime using current nudge
    this.scheduleClick(this.nextBeatTime)

    // Advance nextBeatTime using current effective BPM (reads nudge fresh)
    const spb = this.secondsPerBeat
    this.nextBeatTime += spb

    // Fire setTimeout to schedule the beat after that one, SCHEDULE_AHEAD_MS before it's due
    const now = this.ctx.currentTime
    const msUntilNext = (this.nextBeatTime - now) * 1000 - SCHEDULE_AHEAD_MS
    this.timeoutId = setTimeout(() => this.scheduleNext(), Math.max(0, msUntilNext))
  }

  private scheduleClick(time: number): void {
    const osc = this.ctx.createOscillator()
    const gain = this.ctx.createGain()

    osc.connect(gain)
    gain.connect(this.ctx.destination)

    osc.frequency.setValueAtTime(this._frequency, time)
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
