import { defineStore } from 'pinia'
import { reactive } from 'vue'
import { PulseEngine } from '@renderer/audio/PulseEngine'
import { LoopEngine } from '@renderer/audio/LoopEngine'
import type { LoopRegion } from '@renderer/audio/LoopEngine'

export type DeckId = 'A' | 'B'
export type DeckMode = 'edit' | 'play'

const DEFAULT_BPM = 138
const NUDGE_PERCENT = 4
const BPM_MIN = 60
const BPM_MAX = 200

const PULSE_FREQUENCIES: Record<DeckId, number> = { A: 1000, B: 600 }

function createDeck(id: DeckId) {
  const pulse = new PulseEngine({ bpm: DEFAULT_BPM, frequency: PULSE_FREQUENCIES[id] })
  // LoopEngine shares the same AudioContext as PulseEngine
  const loop = new LoopEngine(pulse.audioContext)

  const state = reactive({
    id,
    mode: 'play' as DeckMode,

    // Pulse (metronome)
    pulseEnabled: true,
    pulsePlaying: false,

    // Loop
    trackLoaded: false,
    loopPlaying: false,
    loopRegion: null as LoopRegion | null,
    loopBeats: 16 as 16 | 32,

    // Shared BPM (drives both pulse and loop playback rate)
    bpm: DEFAULT_BPM,

    nudging: null as 'back' | 'forward' | null,

    getPulseEngine(): PulseEngine { return pulse },
    getLoopEngine(): LoopEngine { return loop },

    get displayBpm(): number {
      return state.bpm
    },

    setBpm(value: number) {
      const clamped = Math.max(BPM_MIN, Math.min(BPM_MAX, value))
      state.bpm = clamped
      pulse.bpm = clamped
      loop.targetBpm = clamped
    },

    async loadTrack(file: File) {
      await loop.loadFile(file)
      state.trackLoaded = true
      state.mode = 'edit'
    },

    setLoopRegion(region: LoopRegion) {
      state.loopRegion = region
      loop.setRegion(region)
      // Sync pulse to inferred BPM so pulse and loop stay phase-locked.
      // Do NOT touch loop.targetBpm — it stays as the user's slider value,
      // so playbackRate = targetBpm / inferredBpm applies the correct pitch shift.
      const inferred = loop.inferredBpm
      pulse.bpm = inferred
      state.bpm = inferred
    },

    setLoopBeats(beats: 16 | 32) {
      state.loopBeats = beats
      loop.setBeats(beats)
      if (state.loopRegion) {
        const r = loop.region
        if (r) state.loopRegion = r
        const inferred = loop.inferredBpm
        pulse.bpm = inferred
        state.bpm = inferred
      }
    },

    togglePlay() {
      const isPlaying = state.pulsePlaying || state.loopPlaying
      if (isPlaying) {
        pulse.stop(); state.pulsePlaying = false
        loop.stop(); state.loopPlaying = false
      } else {
        // Schedule both at the same AudioContext time so they start in sync
        const startTime = pulse.audioContext.currentTime + 0.05
        if (state.pulseEnabled) { pulse.startAt(startTime); state.pulsePlaying = true }
        if (state.trackLoaded && state.loopRegion) { loop.startAt(startTime); state.loopPlaying = true }
      }
    },

    cue() {
      pulse.cue(); state.pulsePlaying = false
      loop.cue(); state.loopPlaying = false
    },

    togglePulse() {
      state.pulseEnabled = !state.pulseEnabled
      if (!state.pulseEnabled && state.pulsePlaying) {
        pulse.stop(); state.pulsePlaying = false
      } else if (state.pulseEnabled && state.loopPlaying) {
        // Re-enable pulse while loop is already playing — start it now in sync
        pulse.startAt(pulse.audioContext.currentTime + 0.05)
        state.pulsePlaying = true
      }
    },

    nudgeStart(direction: 'back' | 'forward') {
      state.nudging = direction
      const offset = direction === 'forward' ? NUDGE_PERCENT : -NUDGE_PERCENT
      pulse.setNudge(offset)
      loop.setNudge(offset)
    },

    nudgeEnd() {
      state.nudging = null
      pulse.setNudge(0)
      loop.setNudge(0)
    },

    get playing(): boolean {
      return state.pulsePlaying || state.loopPlaying
    },

    destroy() {
      pulse.destroy()
      loop.destroy()
    }
  })

  return state
}

export const useDecksStore = defineStore('decks', () => {
  const deckA = createDeck('A')
  const deckB = createDeck('B')

  const decks: Record<DeckId, ReturnType<typeof createDeck>> = { A: deckA, B: deckB }

  function destroy() {
    deckA.destroy()
    deckB.destroy()
  }

  return { deckA, deckB, decks, destroy }
})
