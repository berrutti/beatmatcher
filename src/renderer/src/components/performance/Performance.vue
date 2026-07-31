<template>
  <div class="perf">
    <div class="perf__body">
      <WaveformStrip
        v-if="mixerStore.showWaveformStrip && !collectionStore.bigLibrary"
        class="perf__waveform-strip"
        @scrub-start="onScrubStart"
        @scrub="onScrub"
        @scrub-end="onScrubEnd"
      />

      <div v-if="collectionStore.bigLibrary" class="perf__play perf__play--big-library">
        <div class="perf__big-lib-col">
          <Deck class="perf__compact-deck" :deck="decksStore.deckA" :compact="true" />
          <Deck
            v-if="mixerStore.deckCount === 4"
            class="perf__compact-deck"
            :deck="decksStore.deckC"
            :compact="true"
          />
        </div>
        <div class="perf__big-lib-col">
          <Deck class="perf__compact-deck" :deck="decksStore.deckB" :compact="true" />
          <Deck
            v-if="mixerStore.deckCount === 4"
            class="perf__compact-deck"
            :deck="decksStore.deckD"
            :compact="true"
          />
        </div>
      </div>

      <div v-else class="perf__play">
        <Deck class="perf__deck-a" :deck="decksStore.deckA" />
        <Deck
          class="perf__deck-c"
          :class="{ 'perf__deck--hidden': mixerStore.deckCount === 2 }"
          :deck="decksStore.deckC"
        />
        <div class="perf__center">
          <Mixer />
        </div>
        <Deck class="perf__deck-b" :deck="decksStore.deckB" />
        <Deck
          class="perf__deck-d"
          :class="{ 'perf__deck--hidden': mixerStore.deckCount === 2 }"
          :deck="decksStore.deckD"
        />
      </div>
    </div>

    <Browser class="perf__collection" />
  </div>
</template>

<script setup lang="ts">
import { useDecksStore } from '@renderer/stores/decks';
import type { DeckId } from '@renderer/utils/types';
import { useMixerStore } from '@renderer/stores/mixer';
import { useCollectionStore } from '@renderer/stores/collection';
import Deck from '@renderer/components/deck/Deck.vue';
import Mixer from '@renderer/components/mixer/Mixer.vue';
import Browser from '@renderer/components/collection/Browser.vue';
import WaveformStrip from '@renderer/components/performance/WaveformStrip.vue';

const decksStore = useDecksStore();
const mixerStore = useMixerStore();
const collectionStore = useCollectionStore();

function onScrubStart(deckId: DeckId) {
  mixerStore.startScrubMute(deckId);
}

function onScrub(deckId: DeckId, sec: number) {
  decksStore.decks[deckId].seekTo(sec);
}

function onScrubEnd(deckId: DeckId) {
  mixerStore.endScrubMute(deckId);
}
</script>

<style scoped>
.perf {
  width: 100%;
  height: 100%;
  display: flex;
  flex-direction: column;
}

.perf__body {
  flex: 0 0 auto;
  overflow: hidden;
  display: flex;
  flex-direction: column;
  font-size: 14px;
}

.perf__play {
  display: grid;
  grid-template-columns: minmax(420px, 1fr) minmax(440px, 1fr) minmax(420px, 1fr);
  grid-template-rows: 200px 200px;
  grid-template-areas:
    'deck-a center deck-b'
    'deck-c center deck-d';
}

/* Hidden rather than removed: the second row is what gives the mixer column its
   height, so dropping it collapsed the channel faders and pushed the crossfader
   out of the centre column's overflow. Matches how an inactive mixer channel
   goes, and keeps the decks' canvases at a real size. */
.perf__deck--hidden {
  opacity: 0;
  visibility: hidden;
  transition:
    opacity 0.25s ease,
    visibility 0s linear 0.25s;
}

.perf__waveform-strip {
  /* Each row scales with viewport height (not a flat px value) so the whole
     strip stays proportionate on smaller screens instead of dominating them. */
  flex: 0 0 calc(clamp(30px, 4.2vh, 60px) * v-bind('mixerStore.activeDecks.length'));
  max-height: 240px;
  border-bottom: 1px solid var(--color-border);
}

.perf__play--big-library {
  display: flex;
  flex-direction: row;
}

.perf__big-lib-col {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
}

.perf__big-lib-col + .perf__big-lib-col {
  border-left: 1px solid var(--color-border);
}

.perf__compact-deck {
  flex: 0 0 60px;
  border-bottom: 1px solid var(--color-border);
}

.perf__deck-a {
  grid-area: deck-a;
  min-width: 0;
  border-right: 1px solid var(--color-border);
  border-bottom: 1px solid var(--color-border);
}

.perf__deck-c {
  grid-area: deck-c;
  min-width: 0;
  border-right: 1px solid var(--color-border);
}

.perf__deck-b {
  grid-area: deck-b;
  min-width: 0;
  border-left: 1px solid var(--color-border);
  border-bottom: 1px solid var(--color-border);
}

.perf__deck-d {
  grid-area: deck-d;
  min-width: 0;
  border-left: 1px solid var(--color-border);
}

.perf__center {
  grid-area: center;
  display: flex;
  flex-direction: column;
  min-width: 0;
  overflow: hidden;
  border-left: 1px solid var(--color-border);
  border-right: 1px solid var(--color-border);
}

.perf__collection {
  width: 100%;
  flex: 1;
  min-height: 0;
  border-top: 1px solid var(--color-border);
  overflow: hidden;
  font-size: 13px;
}
</style>
