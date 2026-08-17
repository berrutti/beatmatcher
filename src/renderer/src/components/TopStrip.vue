<template>
  <div class="topstrip">
    <template v-if="appMode.mode === 'performance'">
      <button
        class="btn-secondary topstrip__rec-btn"
        :class="{ 'topstrip__rec-btn--active': mixer.isRecording }"
        tabindex="-1"
        @click="onRecClick"
      >
        {{ $t('topStrip.rec') }}
      </button>

      <button
        class="btn-secondary topstrip__deck-count-btn"
        tabindex="-1"
        @click="mixer.toggleDeckCount()"
      >
        {{ mixer.deckCount === 4 ? $t('topStrip.fourDecks') : $t('topStrip.twoDecks') }}
      </button>

      <button
        class="btn-secondary topstrip__deck-count-btn"
        :class="{ 'topstrip__deck-count-btn--active': mixer.showWaveformStrip }"
        tabindex="-1"
        @click="mixer.toggleWaveformStrip()"
      >
        {{ $t('topStrip.waveforms') }}
      </button>

      <div
        class="topstrip__swarm-btn"
        :class="{ 'topstrip__swarm-btn--active': mixer.swarmMode }"
        v-tooltip="$t('topStrip.swarmHint')"
      >
        {{ $t('topStrip.swarm') }}
        <span
          v-for="deck in DECKS_DISPOSITION"
          :key="deck"
          class="topstrip__swarm-deck"
          :class="{
            'topstrip__swarm-deck--on': mixer.swarmMode && mixer.swarmSelected[deck],
            'topstrip__swarm-deck--inactive': !mixer.activeDecks.includes(deck)
          }"
          >{{ deck }}</span
        >
      </div>
    </template>

    <div class="topstrip__spacer" />

    <span class="topstrip__label">{{ $t('topStrip.vol') }}</span>
    <input
      type="range"
      class="topstrip__master-fader"
      min="0"
      max="1"
      step="0.01"
      :value="mixer.masterGain"
      @input="(e) => mixer.setMasterGain(parseFloat((e.target as HTMLInputElement).value))"
      @dblclick="mixer.setMasterGain(1)"
    />
    <span class="topstrip__master-value">{{ Math.round(mixer.masterGain * 100) }}</span>

    <div class="topstrip__meters">
      <span class="topstrip__meter-label">L</span>
      <div class="topstrip__meter">
        <div class="topstrip__meter-mask" :style="{ width: `${(1 - paramL) * 100}%` }" />
        <div
          v-if="peakL.value > 0"
          class="topstrip__meter-peak"
          :style="{ right: `${(1 - peakL.value) * 100}%` }"
        />
      </div>
      <span class="topstrip__meter-label">R</span>
      <div class="topstrip__meter">
        <div class="topstrip__meter-mask" :style="{ width: `${(1 - paramR) * 100}%` }" />
        <div
          v-if="peakR.value > 0"
          class="topstrip__meter-peak"
          :style="{ right: `${(1 - peakR.value) * 100}%` }"
        />
      </div>
    </div>

    <template v-if="mixer.devicesLoaded">
      <span class="topstrip__label">{{ $t('topStrip.master') }}</span>
      <select
        class="topstrip__select"
        :value="mixer.mainDeviceId"
        @change="(e) => mixer.setMainOutputDevice((e.target as HTMLSelectElement).value, 0)"
      >
        <option value="">{{ $t('topStrip.notConfigured') }}</option>
        <option v-for="d in mixer.outputDevices" :key="d.id" :value="d.id">{{ d.name }}</option>
      </select>
      <select
        v-if="mainDevice && mainDevice.channels > 2"
        class="topstrip__select topstrip__select--ch"
        :value="mixer.mainChannelOffset"
        @change="
          (e) =>
            mixer.setMainOutputDevice(
              mixer.mainDeviceId,
              parseInt((e.target as HTMLSelectElement).value)
            )
        "
      >
        <option v-for="offset in channelPairs(mainDevice.channels)" :key="offset" :value="offset">
          {{ $t('topStrip.channel', { n1: offset + 1, n2: offset + 2 }) }}
        </option>
      </select>

      <template v-if="appMode.mode === 'performance'">
        <span class="topstrip__label topstrip__label--dim">{{ $t('topStrip.cue') }}</span>
        <input
          type="range"
          class="topstrip__cue-mix-fader"
          min="0"
          max="1"
          step="0.01"
          :value="mixer.cueMix"
          @input="(e) => mixer.setCueMix(parseFloat((e.target as HTMLInputElement).value))"
          @dblclick="mixer.setCueMix(0)"
          v-tooltip="$t('topStrip.cueMixHint')"
        />
        <span class="topstrip__label topstrip__label--dim">{{ $t('topStrip.mix') }}</span>
        <select
          class="topstrip__select"
          :value="mixer.cueDeviceId"
          @change="(e) => mixer.setCueOutputDevice((e.target as HTMLSelectElement).value, 0)"
        >
          <option value="">{{ $t('topStrip.notConfigured') }}</option>
          <option v-for="d in mixer.outputDevices" :key="d.id" :value="d.id">{{ d.name }}</option>
        </select>
        <select
          v-if="cueDevice && cueDevice.channels > 2"
          class="topstrip__select topstrip__select--ch"
          :value="mixer.cueChannelOffset"
          @change="
            (e) =>
              mixer.setCueOutputDevice(
                mixer.cueDeviceId,
                parseInt((e.target as HTMLSelectElement).value)
              )
          "
        >
          <option v-for="offset in channelPairs(cueDevice.channels)" :key="offset" :value="offset">
            {{ $t('topStrip.channel', { n1: offset + 1, n2: offset + 2 }) }}
          </option>
        </select>
      </template>

      <button
        class="btn-secondary topstrip__refresh"
        tabindex="-1"
        @click="mixer.loadOutputDevices()"
      >
        ↻
      </button>
      <span v-if="mixer.deviceError" class="topstrip__error">{{ mixer.deviceError }}</span>
    </template>
  </div>
</template>

<script setup lang="ts">
import { computed, ref, watch, onMounted, onUnmounted } from 'vue';
import { useI18n } from 'vue-i18n';
import { useMixerStore } from '@renderer/stores/mixer';
import { DECKS_DISPOSITION, useDecksStore } from '@renderer/stores/decks';
import { useAppModeStore } from '@renderer/stores/appMode';
import { vuParam, smoothParam, stepPeak, type PeakState } from '@renderer/utils/meter';
import { dateStamp } from '@renderer/utils/time';

const { t } = useI18n();
const mixer = useMixerStore();
const decksStore = useDecksStore();
const appMode = useAppModeStore();

const stopMarkPlayedWatch = watch(
  () => DECKS_DISPOSITION.map((id) => decksStore.decks[id].loadedPath),
  (loadedPaths) => {
    for (const path of loadedPaths) {
      if (path) mixer.markPlayed(path);
    }
  }
);

onUnmounted(stopMarkPlayedWatch);

const mainDevice = computed(
  () => mixer.outputDevices.find((d) => d.id === mixer.mainDeviceId) ?? null
);
const cueDevice = computed(
  () => mixer.outputDevices.find((d) => d.id === mixer.cueDeviceId) ?? null
);

function channelPairs(totalChannels: number): number[] {
  const offsets: number[] = [];
  for (let i = 0; i + 1 < totalChannels; i += 2) offsets.push(i);
  return offsets;
}

const paramL = ref(0);
const paramR = ref(0);
const peakL = ref<PeakState>({ value: 0, holdMs: 0 });
const peakR = ref<PeakState>({ value: 0, holdMs: 0 });

let rafId = 0;

async function pollLevels() {
  const [l, r] = await mixer.getMasterLevel();
  const newParamL = vuParam(l);
  const newParamR = vuParam(r);
  paramL.value = smoothParam(paramL.value, newParamL);
  paramR.value = smoothParam(paramR.value, newParamR);
  peakL.value = stepPeak(peakL.value, newParamL);
  peakR.value = stepPeak(peakR.value, newParamR);
  rafId = requestAnimationFrame(pollLevels);
}

async function onRecClick() {
  if (mixer.isRecording) {
    const tempPath = await mixer.stopRecording();
    const destPath = await mixer.pickSavePath(t('files.defaultName', { date: dateStamp() }));
    if (destPath) {
      await mixer.saveRecording(tempPath, destPath);
    } else {
      await mixer.discardRecording(tempPath);
    }
  } else {
    await mixer.startRecording();
  }
}

onMounted(() => {
  mixer.loadOutputDevices();
  rafId = requestAnimationFrame(pollLevels);
});

onUnmounted(() => {
  cancelAnimationFrame(rafId);
});
</script>

<style scoped>
.topstrip {
  width: 100%;
  height: var(--topstrip-h);
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 0 4px;
  border-bottom: 1px solid var(--color-border);
  background: var(--color-bg);
  font-family: var(--font);
  font-size: 11px;
  flex-shrink: 0;
}

.topstrip__meters {
  display: flex;
  align-items: center;
  gap: 4px;
}

.topstrip__meter-label {
  color: var(--color-muted);
  font-size: 9px;
}

.topstrip__meter {
  width: 56px;
  height: 4px;
  background: linear-gradient(
    to right,
    #22c55e 0%,
    #22c55e 65%,
    #facc15 80%,
    var(--color-danger) 92%,
    var(--color-danger) 100%
  );
  border: 0.5px solid var(--color-border);
  border-radius: 1px;
  overflow: hidden;
  position: relative;
}

.topstrip__meter-mask {
  position: absolute;
  top: 0;
  right: 0;
  bottom: 0;
  background: var(--color-surface);
}

.topstrip__meter-peak {
  position: absolute;
  top: 0;
  bottom: 0;
  width: 2px;
  background: #fff;
  transform: translateX(100%);
}

.topstrip__swarm-btn {
  display: flex;
  align-items: center;
  gap: 4px;
  background: transparent;
  border: 1px solid var(--color-border);
  color: var(--color-muted);
  font-family: var(--font);
  font-size: 10px;
  letter-spacing: var(--label-letter-spacing);
  height: 22px;
  padding: 0 8px;
  border-radius: 3px;
  cursor: not-allowed;
  user-select: none;
  text-transform: uppercase;
}

.topstrip__swarm-btn--active {
  border-color: var(--color-accent-amber);
  color: var(--color-accent-amber);
  background: color-mix(in srgb, var(--color-accent-amber) 15%, transparent);
  animation: swarm-pulse 1.2s ease-in-out infinite;
}

@keyframes swarm-pulse {
  0%,
  100% {
    opacity: 1;
  }
  50% {
    opacity: 0.55;
  }
}

.topstrip__swarm-deck {
  display: inline-block;
  width: 9px;
  overflow: hidden;
  text-align: center;
  font-size: 9px;
  font-weight: 700;
  letter-spacing: 0.04em;
  color: var(--color-muted);
  opacity: 0.5;
  transition:
    color 0.25s ease,
    opacity 0.25s ease,
    width 0.25s ease,
    margin 0.25s ease;
}

.topstrip__swarm-deck--on {
  color: var(--color-accent-amber);
  opacity: 1;
}

.topstrip__swarm-deck--inactive {
  width: 0;
  margin-left: -4px;
  opacity: 0;
  visibility: hidden;
  transition:
    opacity 0.25s ease,
    width 0.25s ease,
    margin 0.25s ease,
    visibility 0s linear 0.25s;
}

.topstrip__spacer {
  flex: 1;
}

.topstrip__master-fader {
  -webkit-appearance: none;
  appearance: none;
  width: 72px;
  height: 12px;
  background: transparent;
  cursor: pointer;
  flex-shrink: 0;
}
.topstrip__master-fader::-webkit-slider-runnable-track {
  height: 3px;
  background: #2a2a2a;
  border-radius: 2px;
}
.topstrip__master-fader::-webkit-slider-thumb {
  -webkit-appearance: none;
  appearance: none;
  width: 8px;
  height: 14px;
  background: #e8e8e8;
  border-radius: 2px;
  margin-top: -5.5px;
}

.topstrip__label--dim {
  opacity: 0.5;
}

.topstrip__cue-mix-fader {
  -webkit-appearance: none;
  appearance: none;
  width: 56px;
  height: 12px;
  background: transparent;
  cursor: pointer;
  flex-shrink: 0;
}
.topstrip__cue-mix-fader::-webkit-slider-runnable-track {
  height: 3px;
  background: #2a2a2a;
  border-radius: 2px;
}
.topstrip__cue-mix-fader::-webkit-slider-thumb {
  -webkit-appearance: none;
  appearance: none;
  width: 8px;
  height: 14px;
  background: #e8e8e8;
  border-radius: 2px;
  margin-top: -5.5px;
}

.topstrip__master-value {
  font-size: 9px;
  color: var(--color-muted);
  font-variant-numeric: tabular-nums;
  min-width: 18px;
  text-align: right;
}

.topstrip__label {
  color: var(--color-muted);
  letter-spacing: 0.04em;
  font-size: 9px;
  text-transform: uppercase;
}

.topstrip__select {
  background: var(--color-surface);
  border: 1px solid var(--color-border);
  color: var(--color-text);
  font-family: var(--font);
  font-size: 10px;
  padding: 2px 4px;
  border-radius: 3px;
  cursor: pointer;
  max-width: 140px;
  outline: none;
}

.topstrip__select--ch {
  max-width: 60px;
}

.topstrip__select:focus {
  border-color: #555;
}

.topstrip__rec-btn {
  color: var(--color-muted);
  font-family: var(--font);
  font-size: 10px;
  letter-spacing: var(--label-letter-spacing);
  height: 22px;
  padding: 0 8px;
  border-radius: 3px;
  cursor: pointer;
  text-transform: uppercase;
}

.topstrip__rec-btn:hover {
  border-color: var(--color-danger);
  color: var(--color-danger);
}

.topstrip__rec-btn--active {
  border-color: var(--color-danger);
  color: var(--color-danger);
  background: color-mix(in srgb, var(--color-danger) 15%, transparent);
  animation: rec-pulse 1.2s ease-in-out infinite;
}

@keyframes rec-pulse {
  0%,
  100% {
    opacity: 1;
  }
  50% {
    opacity: 0.55;
  }
}

.topstrip__deck-count-btn {
  color: var(--color-muted);
  font-family: var(--font);
  font-size: 10px;
  letter-spacing: var(--label-letter-spacing);
  height: 22px;
  padding: 0 8px;
  border-radius: 3px;
  cursor: pointer;
  min-width: 0;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  text-transform: uppercase;
}

.topstrip__deck-count-btn:hover {
  border-color: var(--color-text);
  color: var(--color-text);
}

.topstrip__deck-count-btn--active {
  border-color: var(--color-text);
  color: var(--color-text);
}

.topstrip__refresh {
  color: var(--color-muted);
  font-family: var(--font);
  font-size: 12px;
  padding: 1px 6px;
  border-radius: 3px;
  cursor: pointer;
  transition:
    border-color 0.1s,
    color 0.1s;
}

.topstrip__refresh:hover {
  border-color: var(--color-text);
  color: var(--color-text);
}

.topstrip__error {
  color: var(--color-danger);
  font-size: 10px;
}
</style>
