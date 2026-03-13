import type { PulseEngine } from '@renderer/audio/PulseEngine'

/**
 * Returns a function that reads the current beat phase (0.0–1.0)
 * directly from the engine. Components call this inside their own
 * rAF loop — no reactive overhead, no watchers, just a plain function.
 */
export function usePhase(engine: PulseEngine) {
  function getPhase(): number {
    return engine.getPhase()
  }

  function isPlaying(): boolean {
    return engine.playing
  }

  return { getPhase, isPlaying }
}
