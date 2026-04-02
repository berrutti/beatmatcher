import { defineStore } from 'pinia'
import { reactive } from 'vue'
import { PulseEngine } from '@renderer/audio/PulseEngine'
import { LoopEngine } from '@renderer/audio/LoopEngine'
import type { LoopRegion } from '@renderer/audio/LoopEngine'

export type DeckId = 'A' | 'B'
export type DeckMode = 'edit' | 'play'

const DEFAULT_BPM = 138
const NUDGE_PERCENT = 4
export const PITCH_RANGE = 10 // ±10% fine-tune slider range

const PULSE_FREQUENCIES: Record<DeckId, number> = { A: 1000, B: 600 }

// ── Region persistence ─────────────────────────────
type SavedRegion = { startSec: number; endSec: number; beats: 16 | 32 }
const STORAGE_KEY = 'beatmatcher:regions'

function loadSavedRegions(): Record<string, SavedRegion> {
  try { return JSON.parse(localStorage.getItem(STORAGE_KEY) ?? '{}') } catch { return {} }
}

function saveRegion(filename: string, region: SavedRegion) {
  const all = loadSavedRegions()
  all[filename] = region
  localStorage.setItem(STORAGE_KEY, JSON.stringify(all))
}

function getSavedRegion(filename: string): SavedRegion | null {
  return loadSavedRegions()[filename] ?? null
}

// ──────────────────────────────────────────────────

function createDeck(id: DeckId) {
  const pulse = new PulseEngine({ bpm: DEFAULT_BPM, frequency: PULSE_FREQUENCIES[id] })
  const loop = new LoopEngine(pulse.audioContext)

  let currentFilename = ''

  const state = reactive({
    id,
    mode: 'play' as DeckMode,

    pulseEnabled: true,
    pulsePlaying: false,

    trackLoaded: false,
    loopPlaying: false,
    loopRegion: null as LoopRegion | null,
    loopBeats: 16 as 16 | 32,

    // inferredBpm: derived from region (beats / duration). Read-only to the user.
    // targetBpm: what BPM you want to play at. Defaults to inferredBpm, user can override.
    // pitchOffset: ±PITCH_RANGE% fine-tune on top of targetBpm. Slider controls this.
    // Final playbackRate = (targetBpm * (1 + pitchOffset/100)) / inferredBpm
    inferredBpm: DEFAULT_BPM,
    targetBpm: DEFAULT_BPM,
    pitchOffset: 0,

    nudging: null as 'back' | 'forward' | null,
    cueing: false,
    eq: { low: 0, mid: 0, high: 0 },

    getPulseEngine(): PulseEngine { return pulse },
    getLoopEngine(): LoopEngine { return loop },

    // Apply current targetBpm + pitchOffset to both engines
    _applyRate() {
      const effective = state.targetBpm * (1 + state.pitchOffset / 100)
      pulse.bpm = effective
      loop.targetBpm = effective
    },

    setTargetBpm(value: number) {
      state.targetBpm = Math.max(20, value)
      state._applyRate()
    },

    setPitchOffset(pct: number) {
      state.pitchOffset = Math.max(-PITCH_RANGE, Math.min(PITCH_RANGE, pct))
      state._applyRate()
    },

    async loadTrack(file: File) {
      await loop.loadFile(file)
      currentFilename = file.name
      state.trackLoaded = true
      state.mode = 'edit'

      const saved = getSavedRegion(file.name)
      if (saved) {
        state.loopBeats = saved.beats
        state.setLoopRegion(saved)
        state.mode = 'play'
      } else {
        // Auto-place a default region: 16 beats at current BPM, starting at 0.
        const dur = (state.loopBeats / state.inferredBpm) * 60
        state.setLoopRegion({ startSec: 0, endSec: dur, beats: state.loopBeats })
      }
    },

    setLoopRegion(region: LoopRegion) {
      state.loopRegion = region
      loop.setRegion(region)
      state.inferredBpm = loop.inferredBpm
      // Reset targetBpm to match inferred (1x playback) and clear pitch offset
      state.targetBpm = state.inferredBpm
      state.pitchOffset = 0
      state._applyRate()
      if (currentFilename) saveRegion(currentFilename, region)
    },

    setLoopBeats(beats: 16 | 32) {
      state.loopBeats = beats
      loop.setBeats(beats)
      if (state.loopRegion) {
        const r = loop.region
        if (r) state.loopRegion = r
        state.inferredBpm = loop.inferredBpm
        state.targetBpm = state.inferredBpm
        state.pitchOffset = 0
        state._applyRate()
        if (currentFilename) saveRegion(currentFilename, { ...state.loopRegion!, beats })
      }
    },

    togglePlay() {
      if (state.cueing) {
        // CUE→PLAY handoff: just drop the cueing flag, keep playing
        state.cueing = false
        return
      }
      if (state.loopPlaying) {
        pulse.stop(); state.pulsePlaying = false
        loop.stop(); state.loopPlaying = false
      } else {
        const startTime = pulse.audioContext.currentTime + 0.05
        loop.startAt(startTime); state.loopPlaying = true
        if (state.pulseEnabled) { pulse.startAt(startTime); state.pulsePlaying = true }
      }
    },

    cueStart() {
      if (state.cueing || state.loopPlaying) return
      state.cueing = true
      const startTime = pulse.audioContext.currentTime + 0.01
      loop.startAt(startTime); state.loopPlaying = true
      if (state.pulseEnabled) { pulse.startAt(startTime); state.pulsePlaying = true }
    },

    cueEnd() {
      if (!state.cueing) return
      state.cueing = false
      pulse.stop(); state.pulsePlaying = false
      loop.stop(); state.loopPlaying = false
      loop.cue(); pulse.cue()
    },

    togglePulse() {
      state.pulseEnabled = !state.pulseEnabled
      if (state.loopPlaying) {
        if (state.pulseEnabled) {
          const secondsPerBeat = 60 / state.targetBpm
          const phaseInBeat = loop.getPhase()
          const secondsUntilNextBeat = secondsPerBeat * (1 - phaseInBeat)
          const startTime = pulse.audioContext.currentTime + secondsUntilNextBeat
          pulse.startAt(startTime)
          state.pulsePlaying = true
        } else {
          pulse.stop(); state.pulsePlaying = false
        }
      }
    },

    setEq(band: 'low' | 'mid' | 'high', db: number) {
      state.eq[band] = db
      loop.setEq(band, db)
    },

    nudgeStart(direction: 'back' | 'forward') {
      state.nudging = direction
      const offset = direction === 'forward' ? NUDGE_PERCENT : -NUDGE_PERCENT
      pulse.setNudge(offset)
      loop.setNudge(offset)
    },

    nudgeEnd() {
      state.nudging = null
      loop.setNudge(0)
      pulse.setNudge(0)
      // Re-sync pulse to the loop's next beat boundary to fix phase drift
      if (state.loopPlaying && state.pulsePlaying) {
        pulse.stop()
        const secondsPerBeat = 60 / state.targetBpm
        const phaseInBeat = loop.getPhase()
        const secondsUntilNextBeat = secondsPerBeat * (1 - phaseInBeat)
        const startTime = pulse.audioContext.currentTime + secondsUntilNextBeat
        pulse.startAt(startTime)
      }
    },

    // playing = true only when in normal play mode, not cueing
    get playing(): boolean {
      return state.loopPlaying && !state.cueing
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
