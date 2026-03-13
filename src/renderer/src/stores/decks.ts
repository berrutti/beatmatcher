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

function createDeck(id: DeckId) {
  const pulse = new PulseEngine({ bpm: DEFAULT_BPM })
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
    loopBeats: 32 as 16 | 32,

    // Shared BPM (drives both pulse and loop playback rate)
    bpm: DEFAULT_BPM,

    nudging: null as 'back' | 'forward' | null,

    getPulseEngine(): PulseEngine { return pulse },
    getLoopEngine(): LoopEngine { return loop },

    // Inferred from loop region if loaded, otherwise manual BPM
    get displayBpm(): number {
      if (state.trackLoaded && state.loopRegion) return loop.inferredBpm
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
      // Sync BPM from the newly defined loop
      state.bpm = loop.inferredBpm
      pulse.bpm = loop.inferredBpm
    },

    setLoopBeats(beats: 16 | 32) {
      state.loopBeats = beats
      loop.setBeats(beats)
      if (state.loopRegion) {
        // Update stored region reference after LoopEngine scaled it
        const r = loop.region
        if (r) state.loopRegion = r
        state.bpm = loop.inferredBpm
        pulse.bpm = loop.inferredBpm
      }
    },

    togglePlay() {
      const isPlaying = state.pulsePlaying || state.loopPlaying
      if (isPlaying) {
        if (state.pulseEnabled) { pulse.stop(); state.pulsePlaying = false }
        if (state.trackLoaded) { loop.stop(); state.loopPlaying = false }
      } else {
        if (state.pulseEnabled) { pulse.start(); state.pulsePlaying = true }
        if (state.trackLoaded && state.loopRegion) { loop.start(); state.loopPlaying = true }
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
