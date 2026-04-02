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
 *
 * All shortcuts are global — they fire regardless of UI focus or deck mode.
 * Only blocked when typing in an actual text input.
 */

export function useKeyboard() {
  const store = useDecksStore()

  function isTyping(e: KeyboardEvent): boolean {
    const el = e.target as HTMLInputElement
    if (el.tagName === 'TEXTAREA') return true
    if (el.tagName === 'INPUT') {
      const type = el.type.toLowerCase()
      return type === 'text' || type === 'number' || type === 'email' || type === 'search'
    }
    return false
  }

  function onKeyDown(e: KeyboardEvent) {
    if (isTyping(e)) return
    if (e.repeat) return

    const { deckA, deckB } = store
    const key = e.key.toLowerCase()

    // Deck A
    if (key === 'a') { deckA.cueStart(); return }
    if (key === 's') { deckA.togglePlay(); return }
    if (key === 'q') { deckA.nudgeStart('back'); return }
    if (key === 'w') { deckA.nudgeStart('forward'); return }

    // Deck B
    if (key === 'k') { deckB.cueStart(); return }
    if (key === 'l') { deckB.togglePlay(); return }
    if (key === 'i') { deckB.nudgeStart('back'); return }
    if (key === 'o') { deckB.nudgeStart('forward'); return }
  }

  function onKeyUp(e: KeyboardEvent) {
    if (e.key === 'Tab') e.preventDefault()
    if (isTyping(e)) return

    const { deckA, deckB } = store
    const key = e.key.toLowerCase()

    if (key === 'q' || key === 'w') deckA.nudgeEnd()
    if (key === 'i' || key === 'o') deckB.nudgeEnd()
    if (key === 'a') deckA.cueEnd()
    if (key === 'k') deckB.cueEnd()
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
