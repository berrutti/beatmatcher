<template>
  <div class="mixer">
    <div class="mixer__scope">
      <LissajousScope
        :sources="[
          { getPhase: () => decks.deckA.phase, accent: decks.deckA.accent, label: 'A' },
          { getPhase: () => decks.deckB.phase, accent: decks.deckB.accent, label: 'B' }
        ]"
      />
    </div>

    <div class="mixer__channels">
      <div v-for="deckId in (['A', 'B'] as const)" :key="deckId" class="mixer__channel">
        <span class="mixer__channel-label" :style="{ color: decks.decks[deckId].accent }">{{ deckId }}</span>

        <div class="mixer__eq">
          <div v-for="band in (['high', 'mid', 'low'] as const)" :key="band" class="mixer__eq-band">
            <input
              type="range"
              class="mixer__eq-slider"
              :min="EQ_MIN_DB"
              :max="EQ_MAX_DB"
              step="0.5"
              :value="decks.decks[deckId].eq[band]"
              orient="vertical"
              :disabled="!decks.decks[deckId].trackLoaded"
              :style="{ '--eq-accent': decks.decks[deckId].accent }"
              @input="(e) => decks.decks[deckId].setEq(band, parseFloat((e.target as HTMLInputElement).value))"
              @dblclick="decks.decks[deckId].setEq(band, 0)"
            />
            <span class="mixer__eq-label">{{ band[0].toUpperCase() }}</span>
          </div>
        </div>

        <div class="mixer__filter">
          <button
            class="mixer__filter-btn"
            :class="{ 'mixer__filter-btn--active': mixer.filterEnabled[deckId] }"
            :style="{ '--fader-accent': decks.decks[deckId].accent }"
            @click="mixer.toggleFilter(deckId)"
          >
            F
          </button>
          <input
            type="range"
            class="mixer__filter-slider"
            min="-1"
            max="1"
            step="0.01"
            :value="mixer.filter[deckId]"
            :style="{ '--fader-accent': decks.decks[deckId].accent }"
            @input="(e) => mixer.setFilter(deckId, parseFloat((e.target as HTMLInputElement).value))"
            @dblclick="mixer.setFilter(deckId, 0)"
          />
        </div>

        <input
          type="range"
          class="mixer__fader"
          min="0"
          max="1"
          step="0.01"
          :value="mixer.volume[deckId]"
          orient="vertical"
          :style="{ '--fader-accent': decks.decks[deckId].accent }"
          @input="(e) => mixer.setVolume(deckId, parseFloat((e.target as HTMLInputElement).value))"
        />

        <button
          class="mixer__cue-btn"
          :class="{ 'mixer__cue-btn--active': mixer.cueActive[deckId] }"
          @click="mixer.setCueActive(deckId, !mixer.cueActive[deckId])"
        >
          CUE
        </button>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { useDecksStore } from '@renderer/stores/decks';
import { EQ_MIN_DB, EQ_MAX_DB } from '@renderer/stores/decks';
import { useMixerStore } from '@renderer/stores/mixer';
import LissajousScope from '@renderer/components/LissajousScope.vue';

const decks = useDecksStore();
const mixer = useMixerStore();
</script>

<style scoped>
.mixer {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 0.6em;
  padding: 0.6em 0.5em 0.8em;
}

.mixer__scope {
  width: 100%;
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  min-height: 0;
}

.mixer__channels {
  display: flex;
  gap: 2em;
}

.mixer__channel {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 0.4em;
}

.mixer__channel-label {
  font-size: 0.75em;
  font-weight: 700;
  letter-spacing: 0.2em;
}

.mixer__eq {
  display: flex;
  gap: 0.3em;
}

.mixer__eq-band {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 0.2em;
}

.mixer__eq-slider {
  -webkit-appearance: slider-vertical;
  appearance: auto;
  writing-mode: vertical-lr;
  direction: rtl;
  width: 18px;
  height: 4em;
  cursor: pointer;
  accent-color: var(--eq-accent);
}

.mixer__eq-label {
  font-size: 0.5em;
  color: var(--color-muted);
  letter-spacing: 0.1em;
}

.mixer__filter {
  display: flex;
  flex-direction: row;
  align-items: center;
  gap: 0.3em;
}

.mixer__filter-slider {
  width: 4em;
  cursor: pointer;
  accent-color: var(--fader-accent);
}

.mixer__filter-btn {
  background: transparent;
  border: 1px solid var(--color-border);
  color: var(--color-muted);
  font-family: var(--font);
  font-size: 0.55em;
  font-weight: 700;
  letter-spacing: 0.1em;
  padding: 0.25em 0.45em;
  border-radius: 3px;
  cursor: pointer;
  transition: background 0.1s, border-color 0.1s, color 0.1s;
}

.mixer__filter-btn:hover {
  border-color: var(--fader-accent);
  color: var(--fader-accent);
}

.mixer__filter-btn--active {
  border-color: var(--fader-accent);
  color: var(--fader-accent);
  background: color-mix(in srgb, var(--fader-accent) 15%, transparent);
}

.mixer__fader {
  -webkit-appearance: slider-vertical;
  appearance: auto;
  writing-mode: vertical-lr;
  direction: rtl;
  width: 22px;
  height: 6em;
  cursor: pointer;
  accent-color: var(--fader-accent);
}

.mixer__cue-btn {
  background: transparent;
  border: 1px solid var(--color-border);
  color: var(--color-muted);
  font-family: var(--font);
  font-size: 0.6em;
  letter-spacing: 0.15em;
  padding: 0.3em 0.7em;
  border-radius: 3px;
  cursor: pointer;
  transition: background 0.1s, border-color 0.1s, color 0.1s;
}

.mixer__cue-btn:hover {
  border-color: var(--color-cue);
  color: var(--color-cue);
}

.mixer__cue-btn--active {
  border-color: var(--color-cue);
  color: var(--color-cue);
  background: color-mix(in srgb, var(--color-cue) 15%, transparent);
}
</style>
