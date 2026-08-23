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
          <div v-for="spec in mixer.eqSpecs" :key="spec.param" class="mixer__eq-band">
            <input
              type="range"
              class="mixer__eq-slider"
              :min="spec.min"
              :max="spec.max"
              :step="spec.step"
              :value="mixer.paramValue(deckId, keyOf(spec))"
              orient="vertical"
              :style="{ '--eq-accent': decks.decks[deckId].accent }"
              @input="(e) => onInput(deckId, spec, e)"
              @dblclick="onReset(deckId, spec)"
            />
            <span class="mixer__eq-label">{{ spec.param[0].toUpperCase() }}</span>
          </div>
        </div>

        <div class="mixer__filter">
          <button
            class="mixer__filter-btn"
            :class="{ 'mixer__filter-btn--active': mixer.paramActive(deckId, FILTER_ACTIVE) }"
            :style="{ '--fader-accent': decks.decks[deckId].accent }"
            tabindex="-1"
            @click="mixer.toggleParam(deckId, FILTER_ACTIVE)"
          >
            F
          </button>
          <input
            type="range"
            class="mixer__filter-slider"
            :min="mixer.filterSpec.min"
            :max="mixer.filterSpec.max"
            :step="mixer.filterSpec.step"
            :value="mixer.paramValue(deckId, FILTER_VALUE)"
            :style="{ '--fader-accent': decks.decks[deckId].accent }"
            @input="(e) => onInput(deckId, mixer.filterSpec, e)"
            @dblclick="onReset(deckId, mixer.filterSpec)"
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
            :min="mixer.faderSpec.min"
            :max="mixer.faderSpec.max"
            :step="mixer.faderSpec.step"
            :value="mixer.paramValue(deckId, FADER_GAIN)"
            orient="vertical"
            :style="{ '--fader-accent': decks.decks[deckId].accent }"
            @input="(e) => onInput(deckId, mixer.faderSpec, e)"
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
          v-tooltip="$t('mixer.cueHint')"
          tabindex="-1"
          @click="mixer.setCueActive(deckId, !mixer.cueActive[deckId])"
        >
          {{ $t('mixer.cue') }}
        </button>

        <div class="mixer__assign" v-tooltip="$t('mixer.assignHint')">
          <button
            v-for="option in XFADER_ASSIGNS"
            :key="option.value"
            class="mixer__assign-btn"
            :class="{ 'mixer__assign-btn--active': mixer.xfaderAssign[deckId] === option.value }"
            :style="{ '--fader-accent': decks.decks[deckId].accent }"
            :aria-label="$t(option.label)"
            tabindex="-1"
            @click="mixer.toggleXfaderAssign(deckId, option.value)"
          >
            {{ option.short }}
          </button>
        </div>
      </div>
    </div>

    <div class="mixer__xfader">
      <span class="mixer__xfader-end">{{ $t('mixer.assignA') }}</span>
      <input
        type="range"
        class="mixer__xfader-slider"
        min="-1"
        max="1"
        step="0.01"
        :value="mixer.xfaderPosition"
        v-tooltip="$t('mixer.xfaderHint')"
        @input="(e) => mixer.setXfaderPosition(parseFloat((e.target as HTMLInputElement).value))"
        @dblclick="mixer.setXfaderPosition(0)"
      />
      <span class="mixer__xfader-end">{{ $t('mixer.assignB') }}</span>
    </div>
  </div>
</template>

<script setup lang="ts">
import { useDecksStore, DECKS_DISPOSITION } from '@renderer/stores/decks';
import {
  useMixerStore,
  paramKey,
  FADER_GAIN,
  FILTER_VALUE,
  FILTER_ACTIVE,
  type XfaderSide
} from '@renderer/stores/mixer';
import type { DeckId } from '@renderer/utils/types';
import { reactive, watch, onUnmounted } from 'vue';
import { vuParam, smoothParam, stepPeak, type PeakState } from '@renderer/utils/meter';
import type { MixerParamSpec } from '@renderer/utils/sessionCore';

const decks = useDecksStore();
const mixer = useMixerStore();

const XFADER_ASSIGNS: { value: XfaderSide; short: string; label: string }[] = [
  { value: 'a', short: 'A', label: 'mixer.assignA' },
  { value: 'b', short: 'B', label: 'mixer.assignB' }
];

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

function keyOf(spec: MixerParamSpec): string {
  return paramKey(spec.slot, spec.param);
}

function onInput(deckId: DeckId, spec: MixerParamSpec, event: Event) {
  if (!(event.target instanceof HTMLInputElement)) return;
  mixer.swarmAdjust(deckId, keyOf(spec), parseFloat(event.target.value));
}

function onReset(deckId: DeckId, spec: MixerParamSpec) {
  mixer.swarmReset(deckId, keyOf(spec), spec.defaultValue);
}
</script>

<style scoped>
.mixer {
  flex: 1;
  min-width: 0;
  min-height: 0;
  overflow: hidden;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 0.4em;
  padding: 0.3em;
  width: 100%;
}

.mixer__channels {
  display: flex;
  align-items: stretch;
  justify-content: center;
  gap: 0;
  width: 100%;
  min-width: 440px;
  flex: 1;
  min-height: 16em;
}

.mixer__channel {
  flex: 1;
  min-width: 0;
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
  /* Visibility flips only after the fade-out finishes. Becoming active again
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
  letter-spacing: 0.03em;
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
  opacity: var(--disabled-opacity);
  cursor: default;
}

.mixer__eq-slider:disabled::-webkit-slider-thumb {
  cursor: default;
}

.mixer__eq-label {
  font-size: 0.5em;
  color: var(--color-muted);
  letter-spacing: 0.04em;
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
  letter-spacing: 0.04em;
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

.mixer__assign {
  display: flex;
  gap: 1px;
  margin-top: 0.35em;
}

.mixer__assign-btn {
  background: transparent;
  border: 1px solid var(--color-border);
  color: var(--color-muted);
  font-family: var(--font);
  font-size: 0.55em;
  font-weight: 700;
  letter-spacing: 0.04em;
  padding: 0.25em 0.45em;
  border-radius: 3px;
  cursor: pointer;
  transition:
    background 0.1s,
    border-color 0.1s,
    color 0.1s;
}

.mixer__assign-btn:hover {
  border-color: var(--fader-accent);
  color: var(--fader-accent);
}

.mixer__assign-btn--active {
  border-color: var(--fader-accent);
  color: var(--fader-accent);
  background: color-mix(in srgb, var(--fader-accent) 15%, transparent);
}

.mixer__xfader {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 0.6em;
  padding: 0.5em 0.8em 0.2em;
}

.mixer__xfader-end {
  font-size: 0.5em;
  color: var(--color-muted);
  letter-spacing: 0.04em;
}

/* The channel fader's cap and track, laid on its side: a crossfader is a fader,
   not a sweep, so it reads as one rather than borrowing the filter's knurl. */
.mixer__xfader-slider {
  -webkit-appearance: none;
  appearance: none;
  width: 100%;
  max-width: 16em;
  height: 28px;
  cursor: pointer;
  background: transparent;
  padding: 0;
}

.mixer__xfader-slider::-webkit-slider-runnable-track {
  height: 4px;
  background: #111;
  border: 1px solid #282828;
  border-radius: 2px;
}

.mixer__xfader-slider::-webkit-slider-thumb {
  -webkit-appearance: none;
  appearance: none;
  width: 20px;
  height: 28px;
  background: linear-gradient(
    to right,
    #303030 0%,
    #303030 38%,
    #be1c1c 38%,
    #be1c1c 62%,
    #303030 62%,
    #303030 100%
  );
  border-radius: 2px;
  border-left: 1px solid #555;
  border-right: 1px solid #1a1a1a;
  border-top: 1px solid #444;
  border-bottom: 1px solid #444;
  cursor: grab;
  margin-top: -11px;
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
  font-weight: 600;
  letter-spacing: 0.02em;
  padding: 0.3em 0.7em;
  border-radius: 3px;
  cursor: pointer;
  transition:
    background 0.1s,
    border-color 0.1s,
    color 0.1s;
}

.mixer__cue-btn:hover:not(:disabled) {
  border-color: var(--color-cue);
  color: var(--color-cue);
}

.mixer__cue-btn:disabled {
  opacity: var(--disabled-opacity);
  cursor: default;
}

.mixer__cue-btn--active {
  border-color: var(--color-cue);
  color: var(--color-cue);
  background: color-mix(in srgb, var(--color-cue) 15%, transparent);
}
</style>
