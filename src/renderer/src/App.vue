<template>
  <div class="app">
    <div class="app__stage">
      <main class="app__decks">
        <DeckPanel deck-id="A" />
        <div class="app__center">
          <LissajousScope
            :sources="[
              { getPhase: () => store.deckA.phase, accent: store.deckA.accent, label: 'A' },
              { getPhase: () => store.deckB.phase, accent: store.deckB.accent, label: 'B' },
            ]"
          />
        </div>
        <DeckPanel deck-id="B" />
      </main>


      <Transition name="kb">
        <div v-if="showKeys" class="app__keybindings">
          <div class="app__kb-group">
            <span class="app__kb-title">DECK A</span>
            <div class="app__kb-row"><kbd>A</kbd> cue</div>
            <div class="app__kb-row"><kbd>S</kbd> play / pause</div>
            <div class="app__kb-row"><kbd>Q</kbd> nudge ◀</div>
            <div class="app__kb-row"><kbd>W</kbd> nudge ▶</div>
          </div>
          <div class="app__kb-group">
            <span class="app__kb-title">DECK B</span>
            <div class="app__kb-row"><kbd>K</kbd> cue</div>
            <div class="app__kb-row"><kbd>L</kbd> play / pause</div>
            <div class="app__kb-row"><kbd>I</kbd> nudge ◀</div>
            <div class="app__kb-row"><kbd>O</kbd> nudge ▶</div>
          </div>
        </div>
      </Transition>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted, onUnmounted } from 'vue'
import { useDecksStore } from '@renderer/stores/decks'
import { useKeyboard } from '@renderer/composables/useKeyboard'
import DeckPanel from '@renderer/components/DeckPanel.vue'
import LissajousScope from '@renderer/components/LissajousScope.vue'

useKeyboard()

const store = useDecksStore()
onUnmounted(() => store.destroy())

const showKeys = ref(false)

function onKeyDown(e: KeyboardEvent) {
  if (e.key === '?' && !e.repeat) showKeys.value = true
}
function onKeyUp(e: KeyboardEvent) {
  if (e.key === '?') showKeys.value = false
}

onMounted(() => {
  window.addEventListener('keydown', onKeyDown)
  window.addEventListener('keyup', onKeyUp)
})
onUnmounted(() => {
  window.removeEventListener('keydown', onKeyDown)
  window.removeEventListener('keyup', onKeyUp)
})
</script>
