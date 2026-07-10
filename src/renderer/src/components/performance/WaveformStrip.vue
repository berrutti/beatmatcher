<template>
  <WaveformStrips
    class="waveform-strip"
    :sources="sources"
    @scrub-start="(i) => emit('scrub-start', deckIds[i])"
    @scrub="(i, sec) => emit('scrub', deckIds[i], sec)"
    @scrub-end="(i) => emit('scrub-end', deckIds[i])"
  />
</template>

<script setup lang="ts">
import { computed } from 'vue';
import { useDecksStore, type DeckId } from '@renderer/stores/decks';
import { useMixerStore } from '@renderer/stores/mixer';
import WaveformStrips from '@renderer/components/mixer/Waveform.vue';

const emit = defineEmits<{
  'scrub-start': [deckId: DeckId];
  scrub: [deckId: DeckId, sec: number];
  'scrub-end': [deckId: DeckId];
}>();

const decksStore = useDecksStore();
const mixerStore = useMixerStore();

const deckIds = computed(() => mixerStore.activeDecks);

const sources = computed(() =>
  deckIds.value.map((id) => {
    const deck = decksStore.decks[id];
    return {
      getPosition: () => deck.getPlayheadPosition(),
      getBpm: () => deck.trackBpm,
      getBeatOffset: () => deck.beatOffset,
      getRate: () => deck.rate,
      getDenseData: () => deck.denseSpectralData,
      getDenseRate: () => deck.denseSpectralRate,
      isWaveformLoading: () => deck.waveformLoading,
      accent: deck.accent
    };
  })
);
</script>

<style scoped>
.waveform-strip {
  width: 100%;
  height: 100%;
}
</style>
