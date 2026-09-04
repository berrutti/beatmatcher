<template>
  <WaveformStrips
    class="waveform-strip"
    :sources="sources"
    :waveform-style="settings.waveformStyle"
    @scrub-start="(i) => emit('scrub-start', deckIds[i])"
    @scrub="(i, sec) => emit('scrub', deckIds[i], sec)"
    @scrub-end="(i) => emit('scrub-end', deckIds[i])"
  />
</template>

<script setup lang="ts">
import { computed } from 'vue';
import { useDecksStore } from '@renderer/stores/decks';
import type { DeckId } from '@renderer/utils/types';
import { useMixerStore } from '@renderer/stores/mixer';
import { useSettingsStore } from '@renderer/stores/settings';
import WaveformStrips from '@renderer/components/mixer/Waveform.vue';

const emit = defineEmits<{
  'scrub-start': [deckId: DeckId];
  scrub: [deckId: DeckId, sec: number];
  'scrub-end': [deckId: DeckId];
}>();

const decksStore = useDecksStore();
const mixerStore = useMixerStore();
const settings = useSettingsStore();

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
      getDensePointsReady: () => deck.densePointsReady,
      getBandReference: () => deck.bandReference,
      isWaveformLoading: () => deck.waveformLoading,
      getLoopRegion: () => deck.loopRegion,
      getLoopActive: () => deck.loopActive,
      getCuePoint: () => deck.cuePoint,
      getBandBalance: () => deck.bandBalance
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
