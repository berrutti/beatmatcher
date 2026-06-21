<template>
  <div class="mixer">
    <div class="mixer__channels">
      <div
        v-for="deckId in DECKS_DISPOSITION"
        :key="deckId"
        class="mixer__channel"
        :class="[
          swarmChannelClass(deckId),
          { 'mixer__channel--inactive': !mixer.activeDecks.includes(deckId) }
        ]"
      >
        <span class="mixer__channel-label" :style="{ color: decks.decks[deckId].accent }">{{
          deckId
        }}</span>

        <div class="mixer__eq">
          <div v-for="band in ['low', 'mid', 'high'] as const" :key="band" class="mixer__eq-band">
            <input
              type="range"
              class="mixer__eq-slider"
              :min="EQ_MIN_DB"
              :max="EQ_MAX_DB"
              step="0.5"
              :value="mixer.eq[deckId][band]"
              orient="vertical"
              :style="{ '--eq-accent': decks.decks[deckId].accent }"
              @input="
                (e) => onEqInput(deckId, band, parseFloat((e.target as HTMLInputElement).value))
              "
              @dblclick="onEqReset(deckId, band)"
            />
            <span class="mixer__eq-label">{{ band[0].toUpperCase() }}</span>
          </div>
        </div>

        <div class="mixer__filter">
          <button
            class="mixer__filter-btn"
            :class="{ 'mixer__filter-btn--active': mixer.filterEnabled[deckId] }"
            :style="{ '--fader-accent': decks.decks[deckId].accent }"
            tabindex="-1"
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
            @input="(e) => onFilterInput(deckId, parseFloat((e.target as HTMLInputElement).value))"
            @dblclick="onFilterReset(deckId)"
          />
          <button
            class="mixer__filter-btn mixer__filter-btn--ghost"
            aria-hidden="true"
            tabindex="-1"
          >
            F
          </button>
        </div>

        <div class="mixer__fader-row">
          <div class="mixer__ghost_meter"></div>
          <input
            type="range"
            class="mixer__fader"
            min="0"
            max="1"
            step="0.01"
            :value="mixer.volume[deckId]"
            orient="vertical"
            :style="{ '--fader-accent': decks.decks[deckId].accent }"
            @input="(e) => onVolumeInput(deckId, parseFloat((e.target as HTMLInputElement).value))"
          />
          <div class="mixer__meter">
            <div
              class="mixer__meter-mask"
              :style="{ height: `${(1 - deckParams[deckId]) * 100}%` }"
            />
            <div
              v-if="deckPeaks[deckId].value > 0"
              class="mixer__meter-peak"
              :style="{ bottom: `${deckPeaks[deckId].value * 100}%` }"
            />
          </div>
        </div>

        <button
          class="mixer__cue-btn"
          :class="{ 'mixer__cue-btn--active': mixer.cueActive[deckId] }"
          :disabled="mixer.swarmMode"
          :title="$t('mixer.cueHint')"
          tabindex="-1"
          @click="mixer.setCueActive(deckId, !mixer.cueActive[deckId])"
        >
          {{ $t('mixer.cue') }}
        </button>
      </div>
    </div>

    <WaveformStrips
      class="mixer__wrapper"
      :sources="[
        {
          getPosition: () => decks.deckC.getPlayheadPosition(),
          getBpm: () => decks.deckC.trackBpm,
          getBeatOffset: () => decks.deckC.beatOffset,
          getRate: () =>
            decks.deckC.trackBpm && decks.deckC.targetBpm
              ? decks.deckC.targetBpm / decks.deckC.trackBpm
              : 1,
          getDenseData: () => decks.deckC.denseSpectralData,
          getDenseRate: () => decks.deckC.denseSpectralRate,
          isWaveformLoading: () => decks.deckC.waveformLoading,
          accent: decks.deckC.accent
        },
        {
          getPosition: () => decks.deckA.getPlayheadPosition(),
          getBpm: () => decks.deckA.trackBpm,
          getBeatOffset: () => decks.deckA.beatOffset,
          getRate: () =>
            decks.deckA.trackBpm && decks.deckA.targetBpm
              ? decks.deckA.targetBpm / decks.deckA.trackBpm
              : 1,
          getDenseData: () => decks.deckA.denseSpectralData,
          getDenseRate: () => decks.deckA.denseSpectralRate,
          isWaveformLoading: () => decks.deckA.waveformLoading,
          accent: decks.deckA.accent
        },
        {
          getPosition: () => decks.deckB.getPlayheadPosition(),
          getBpm: () => decks.deckB.trackBpm,
          getBeatOffset: () => decks.deckB.beatOffset,
          getRate: () =>
            decks.deckB.trackBpm && decks.deckB.targetBpm
              ? decks.deckB.targetBpm / decks.deckB.trackBpm
              : 1,
          getDenseData: () => decks.deckB.denseSpectralData,
          getDenseRate: () => decks.deckB.denseSpectralRate,
          isWaveformLoading: () => decks.deckB.waveformLoading,
          accent: decks.deckB.accent
        },
        {
          getPosition: () => decks.deckD.getPlayheadPosition(),
          getBpm: () => decks.deckD.trackBpm,
          getBeatOffset: () => decks.deckD.beatOffset,
          getRate: () =>
            decks.deckD.trackBpm && decks.deckD.targetBpm
              ? decks.deckD.targetBpm / decks.deckD.trackBpm
              : 1,
          getDenseData: () => decks.deckD.denseSpectralData,
          getDenseRate: () => decks.deckD.denseSpectralRate,
          isWaveformLoading: () => decks.deckD.waveformLoading,
          accent: decks.deckD.accent
        }
      ]"
      @scrub-start="onScrubStart"
      @scrub="onScrub"
      @scrub-end="onScrubEnd"
    />
  </div>
</template>

<script setup lang="ts">
import { useDecksStore, DECKS_DISPOSITION } from '@renderer/stores/decks';
import { useMixerStore, EQ_MIN_DB, EQ_MAX_DB } from '@renderer/stores/mixer';
import WaveformStrips from '@renderer/components/mixer/Waveform.vue';
import type { DeckId } from '@renderer/stores/decks';
import { reactive, watch, onUnmounted } from 'vue';
import { vuParam, smoothParam, stepPeak, type PeakState } from '@renderer/utils/meter';

const decks = useDecksStore();
const mixer = useMixerStore();

let scrubSavedVolume: number | null = null;

function onScrubStart(sourceIndex: number) {
  const deckId = DECKS_DISPOSITION[sourceIndex];
  if (!deckId) return;
  scrubSavedVolume = mixer.volume[deckId];
  mixer.setVolume(deckId, 0);
}

function onScrub(sourceIndex: number, sec: number) {
  const deckId = DECKS_DISPOSITION[sourceIndex];
  if (!deckId) return;
  decks.decks[deckId].seekTo(sec);
}

function onScrubEnd(sourceIndex: number) {
  const deckId = DECKS_DISPOSITION[sourceIndex];
  if (!deckId || scrubSavedVolume === null) return;
  mixer.setVolume(deckId, scrubSavedVolume);
  scrubSavedVolume = null;
}

const deckParams = reactive<Record<DeckId, number>>({ A: 0, B: 0, C: 0, D: 0, E: 0 });
const deckPeaks = reactive<Record<DeckId, PeakState>>({
  A: { value: 0, holdMs: 0 },
  B: { value: 0, holdMs: 0 },
  C: { value: 0, holdMs: 0 },
  D: { value: 0, holdMs: 0 },
  E: { value: 0, holdMs: 0 }
});
let levelPollId: ReturnType<typeof setInterval> | null = null;

function startLevelPoll() {
  if (levelPollId !== null) return;
  levelPollId = setInterval(async () => {
    const levels = await mixer.getDeckLevels();
    for (const id of Object.keys(levels) as DeckId[]) {
      const [l, r] = levels[id];
      const newParam = vuParam(Math.max(l, r));
      deckParams[id] = smoothParam(deckParams[id], newParam);
      deckPeaks[id] = stepPeak(deckPeaks[id], newParam);
    }
  }, 33);
}

function stopLevelPoll() {
  if (levelPollId !== null) {
    clearInterval(levelPollId);
    levelPollId = null;
  }
  for (const id of Object.keys(deckParams) as DeckId[]) {
    deckParams[id] = 0;
    deckPeaks[id] = { value: 0, holdMs: 0 };
  }
}

watch(
  () => decks.anyDeckActive,
  (active) => {
    if (active) startLevelPoll();
    else stopLevelPoll();
  }
);

onUnmounted(stopLevelPoll);

function swarmChannelClass(deckId: DeckId) {
  if (!mixer.swarmMode || !mixer.swarmSelected[deckId]) return {};
  const decklist = mixer.activeDecks;
  const idx = decklist.indexOf(deckId);
  return {
    'mixer__channel--swarm-selected': true,
    'mixer__channel--swarm-no-left': idx > 0 && mixer.swarmSelected[decklist[idx - 1]],
    'mixer__channel--swarm-no-right':
      idx < decklist.length - 1 && mixer.swarmSelected[decklist[idx + 1]]
  };
}

function swarmAffected(deckId: DeckId): DeckId[] {
  const selected = mixer.activeDecks.filter((ch) => mixer.swarmSelected[ch]);
  if (!selected.includes(deckId)) selected.push(deckId);
  return selected;
}

function onVolumeInput(deckId: DeckId, newVal: number) {
  if (mixer.swarmMode) {
    const delta = newVal - mixer.volume[deckId];
    for (const ch of swarmAffected(deckId)) mixer.setVolume(ch, mixer.volume[ch] + delta);
  } else {
    mixer.setVolume(deckId, newVal);
  }
}

function onFilterInput(deckId: DeckId, newVal: number) {
  if (mixer.swarmMode) {
    const delta = newVal - mixer.filter[deckId];
    for (const ch of swarmAffected(deckId)) mixer.setFilter(ch, mixer.filter[ch] + delta);
  } else {
    mixer.setFilter(deckId, newVal);
  }
}

function onFilterReset(deckId: DeckId) {
  if (mixer.swarmMode) {
    for (const ch of swarmAffected(deckId)) mixer.setFilter(ch, 0);
  } else {
    mixer.setFilter(deckId, 0);
  }
}

function onEqInput(deckId: DeckId, band: 'high' | 'mid' | 'low', newVal: number) {
  if (mixer.swarmMode) {
    const delta = newVal - mixer.eq[deckId][band];
    for (const ch of swarmAffected(deckId)) mixer.setEq(ch, band, mixer.eq[ch][band] + delta);
  } else {
    mixer.setEq(deckId, band, newVal);
  }
}

function onEqReset(deckId: DeckId, band: 'high' | 'mid' | 'low') {
  if (mixer.swarmMode) {
    for (const ch of swarmAffected(deckId)) mixer.setEq(ch, band, 0);
  } else {
    mixer.setEq(deckId, band, 0);
  }
}
</script>

<style scoped>
.mixer {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 0.4em;
  padding: 0.3em 0.3em 0em;
  width: 100%;
}

.mixer__wrapper {
  flex: 1;
  min-height: 0;
  width: 100%;
}

.mixer__channels {
  display: flex;
  align-items: stretch;
  justify-content: center;
  gap: 0.4em;
  width: 100%;
  flex: 1;
  min-height: 16em;
}

.mixer__channel {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 0.25em;
  border-radius: 4px;
  border: 1px solid transparent;
  opacity: 1;
  transition:
    background 0.1s,
    border-color 0.1s,
    opacity 0.25s ease;
}

.mixer__channel--inactive {
  /* Visibility flips only after the fade-out finishes; becoming active again
     flips it immediately (no delay declared here) so the fade-in is visible
     from the start. */
  opacity: 0;
  visibility: hidden;
  transition:
    opacity 0.25s ease,
    visibility 0s linear 0.25s;
}

.mixer__channel--swarm-selected {
  position: relative;
  z-index: 1;
}

/* The whole highlight (fill + outline) lives on ::before, sitting behind the
   channel's controls (z-index: -1) so a right-connected channel can stretch it
   across the inter-channel gap to its neighbor without tinting the sliders. */
.mixer__channel--swarm-selected::before {
  content: '';
  position: absolute;
  inset: 0;
  z-index: -1;
  background: color-mix(in srgb, var(--color-accent-amber) 8%, transparent);
  border: 1px solid color-mix(in srgb, var(--color-accent-amber) 40%, transparent);
  border-radius: 4px;
  pointer-events: none;
}

/* Adjacent selected channels read as one rounded group. The channel whose
   right neighbor is also selected stretches its highlight across the column gap
   (0.4em, see .mixer__channels) to meet that neighbor and drops the touching
   border + corners; the left neighbor only drops its touching border + corners,
   so the two halves join seamlessly with rounded ends. */
.mixer__channel--swarm-no-right::before {
  right: -0.4em;
  border-right: none;
  border-top-right-radius: 0;
  border-bottom-right-radius: 0;
}

.mixer__channel--swarm-no-left::before {
  border-left: none;
  border-top-left-radius: 0;
  border-bottom-left-radius: 0;
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
  -webkit-appearance: none;
  appearance: none;
  writing-mode: vertical-lr;
  direction: rtl;
  width: 20px;
  height: 9em;
  cursor: pointer;
  background: transparent;
  padding: 0;
}

.mixer__eq-slider::-webkit-slider-runnable-track {
  width: 3px;
  background: #161616;
  border: 1px solid #2c2c2c;
  border-radius: 1px;
}

.mixer__eq-slider::-webkit-slider-thumb {
  -webkit-appearance: none;
  appearance: none;
  width: 18px;
  height: 14px;
  background:
    repeating-linear-gradient(
      to bottom,
      rgba(0, 0, 0, 0.35) 0px,
      rgba(0, 0, 0, 0.35) 1px,
      transparent 1px,
      transparent 3px
    ),
    linear-gradient(to right, #4a4a4a, #808080 30%, #808080 70%, #4a4a4a);
  border-radius: 1px;
  border-top: 1px solid #999;
  border-bottom: 1px solid #2a2a2a;
  border-left: 1px solid #5a5a5a;
  border-right: 1px solid #5a5a5a;
  cursor: grab;
  margin-left: -7.5px;
  box-shadow: 0 1px 4px rgba(0, 0, 0, 0.6);
}

.mixer__eq-slider:disabled {
  opacity: 0.35;
  cursor: default;
}

.mixer__eq-slider:disabled::-webkit-slider-thumb {
  cursor: default;
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
  -webkit-appearance: none;
  appearance: none;
  width: 6.5em;
  height: 18px;
  cursor: pointer;
  background: transparent;
  padding: 0;
}

.mixer__filter-slider::-webkit-slider-runnable-track {
  height: 3px;
  background: #161616;
  border: 1px solid #2c2c2c;
  border-radius: 1px;
}

.mixer__filter-slider::-webkit-slider-thumb {
  -webkit-appearance: none;
  appearance: none;
  width: 13px;
  height: 18px;
  background:
    repeating-linear-gradient(
      to right,
      rgba(0, 0, 0, 0.35) 0px,
      rgba(0, 0, 0, 0.35) 1px,
      transparent 1px,
      transparent 3px
    ),
    linear-gradient(to bottom, #4a4a4a, #808080 30%, #808080 70%, #4a4a4a);
  border-radius: 1px;
  border-left: 1px solid #999;
  border-right: 1px solid #2a2a2a;
  border-top: 1px solid #5a5a5a;
  border-bottom: 1px solid #5a5a5a;
  cursor: grab;
  margin-top: -8px;
  box-shadow: 0 1px 4px rgba(0, 0, 0, 0.6);
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
  transition:
    background 0.1s,
    border-color 0.1s,
    color 0.1s;
}

.mixer__filter-btn:hover {
  border-color: var(--fader-accent);
  color: var(--fader-accent);
}

.mixer__filter-btn--ghost {
  visibility: hidden;
  pointer-events: none;
}

.mixer__filter-btn--active {
  border-color: var(--fader-accent);
  color: var(--fader-accent);
  background: color-mix(in srgb, var(--fader-accent) 15%, transparent);
}

.mixer__fader-row {
  display: flex;
  flex-direction: row;
  align-items: stretch;
  gap: 3px;
  flex: 1;
  min-height: 0;
}

.mixer__ghost_meter {
  width: 5px;
  height: 100%;
  position: relative;
  overflow: hidden;
}

.mixer__meter {
  width: 5px;
  height: 100%;
  background: linear-gradient(
    to top,
    #22c55e 0%,
    #22c55e 65%,
    #facc15 80%,
    var(--color-danger) 92%,
    var(--color-danger) 100%
  );
  border-radius: 2px;
  position: relative;
  overflow: hidden;
}

.mixer__meter-mask {
  position: absolute;
  top: 0;
  left: 0;
  right: 0;
  background: #000;
}

.mixer__meter-peak {
  position: absolute;
  left: 0;
  right: 0;
  height: 2px;
  background: #fff;
  transform: translateY(100%);
}

.mixer__fader {
  -webkit-appearance: none;
  appearance: none;
  writing-mode: vertical-lr;
  direction: rtl;
  width: 30px;
  height: 100%;
  cursor: pointer;
  background: transparent;
  padding: 0;
}

.mixer__fader::-webkit-slider-runnable-track {
  width: 4px;
  background: #111;
  border: 1px solid #282828;
  border-radius: 2px;
}

.mixer__fader::-webkit-slider-thumb {
  -webkit-appearance: none;
  appearance: none;
  width: 28px;
  height: 20px;
  background: linear-gradient(
    to bottom,
    #303030 0%,
    #303030 38%,
    #be1c1c 38%,
    #be1c1c 62%,
    #303030 62%,
    #303030 100%
  );
  border-radius: 2px;
  border-top: 1px solid #555;
  border-bottom: 1px solid #1a1a1a;
  border-left: 1px solid #444;
  border-right: 1px solid #444;
  cursor: grab;
  margin-left: -14px;
  box-shadow:
    0 3px 7px rgba(0, 0, 0, 0.8),
    inset 0 1px 0 rgba(255, 255, 255, 0.06);
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
  transition:
    background 0.1s,
    border-color 0.1s,
    color 0.1s;
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
