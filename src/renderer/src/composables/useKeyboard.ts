import { onMounted, onUnmounted } from 'vue'
import { useDecksStore } from '@renderer/stores/decks'

/**
 * Keyboard layout:
 *
 * Deck A (left)       Deck B (right)
 * ─────────────────   ─────────────────
 * Q  nudge back       O  nudge back
 * W  nudge forward    P  nudge forward
 * A  play/pause       K  play/pause
 * S  cue              L  cue
 * TAB pulse toggle    '  pulse toggle
 *
 * All shortcuts are global — they fire regardless of UI focus or deck mode.
 * Only blocked when typing in an actual text input.
 */

export function useKeyboard() {
  const store = useDecksStore()

  function isTyping(e: KeyboardEvent): boolean {
    const tag = (e.target as HTMLElement).tagName
    return tag === 'INPUT' || tag === 'TEXTAREA'
  }

  function onKeyDown(e: KeyboardEvent) {
    if (isTyping(e)) return
    if (e.repeat) return

    const { deckA, deckB } = store
    const key = e.key.toLowerCase()

    // Deck A
    if (key === 'a') { deckA.togglePlay(); return }
    if (key === 's') { deckA.cue(); return }
    if (key === 'q') { deckA.nudgeStart('back'); return }
    if (key === 'w') { deckA.nudgeStart('forward'); return }
    if (e.key === 'Tab') { e.preventDefault(); deckA.togglePulse(); return }

    // Deck B
    if (key === 'k') { deckB.togglePlay(); return }
    if (key === 'l') { deckB.cue(); return }
    if (key === 'o') { deckB.nudgeStart('back'); return }
    if (key === 'p') { deckB.nudgeStart('forward'); return }
    if (key === "'") { deckB.togglePulse(); return }
  }

  function onKeyUp(e: KeyboardEvent) {
    if (isTyping(e)) return

    const { deckA, deckB } = store
    const key = e.key.toLowerCase()

    if (key === 'q' || key === 'w') deckA.nudgeEnd()
    if (key === 'o' || key === 'p') deckB.nudgeEnd()
  }

  onMounted(() => {
    window.addEventListener('keydown', onKeyDown)
    window.addEventListener('keyup', onKeyUp)
  })

  onUnmounted(() => {
    window.removeEventListener('keydown', onKeyDown)
    window.removeEventListener('keyup', onKeyUp)
  })
}
