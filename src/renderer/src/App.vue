<template>
  <div class="app">
    <header class="app__header">
      <h1 class="app__title">BEATMATCHER</h1>
      <p class="app__subtitle">Beat Matching Trainer</p>
    </header>

    <main class="app__decks">
      <DeckPanel deck-id="A" />
      <div class="app__center">
        <LissajousScope />
      </div>
      <DeckPanel deck-id="B" />
    </main>

    <footer class="app__keybindings">
      <div class="app__kb-group">
        <span class="app__kb-title">DECK A</span>
        <kbd>Q</kbd> nudge ◀
        <kbd>W</kbd> nudge ▶
        <kbd>A</kbd> play/pause
        <kbd>S</kbd> cue
      </div>
      <div class="app__kb-group">
        <span class="app__kb-title">DECK B</span>
        <kbd>O</kbd> nudge ◀
        <kbd>P</kbd> nudge ▶
        <kbd>K</kbd> play/pause
        <kbd>L</kbd> cue
      </div>
    </footer>
  </div>
</template>

<script setup lang="ts">
import { onUnmounted } from 'vue'
import { useDecksStore } from '@renderer/stores/decks'
import { useKeyboard } from '@renderer/composables/useKeyboard'
import DeckPanel from '@renderer/components/DeckPanel.vue'
import LissajousScope from '@renderer/components/LissajousScope.vue'

useKeyboard()

const store = useDecksStore()
onUnmounted(() => store.destroy())
</script>
