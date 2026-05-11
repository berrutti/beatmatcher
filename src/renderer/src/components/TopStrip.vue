<template>
  <div class="topstrip">
    <button
      class="topstrip__rec-btn"
      :class="{ 'topstrip__rec-btn--active': isRecording }"
      :title="isRecording ? 'Stop recording' : 'Record master output'"
      @click="onRecClick"
    >
      REC
    </button>

    <button
      class="topstrip__edit-btn"
      :class="{ 'topstrip__edit-btn--active': editMode }"
      @click="emit('toggle-edit')"
    >
      EDIT
    </button>

    <button class="topstrip__deck-count-btn" @click="mixer.toggleDeckCount()">
      {{ mixer.deckCount === 4 ? '4 DECKS' : '2 DECKS' }}
    </button>

    <div
      class="topstrip__swarm-btn"
      :class="{ 'topstrip__swarm-btn--active': mixer.swarmMode }"
      title="Activate with CapsLock"
    >
      SWARM
      <span
        v-for="deck in activeDecks"
        :key="deck"
        class="topstrip__swarm-deck"
        :class="{ 'topstrip__swarm-deck--on': mixer.swarmMode && mixer.swarmSelected[deck] }"
        >{{ deck }}</span
      >
    </div>

    <div class="topstrip__spacer" />

    <span class="topstrip__label">VOL</span>
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
      <span class="topstrip__label">MASTER</span>
      <select
        class="topstrip__select"
        :value="mixer.mainDeviceId"
        @change="(e) => mixer.setMainOutputDevice((e.target as HTMLSelectElement).value, 0)"
      >
        <option value="">Not configured</option>
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
          Ch {{ offset + 1 }}-{{ offset + 2 }}
        </option>
      </select>

      <span class="topstrip__label topstrip__label--dim">CUE</span>
      <input
        type="range"
        class="topstrip__cue-mix-fader"
        min="0"
        max="1"
        step="0.01"
        :value="mixer.cueMix"
        @input="(e) => mixer.setCueMix(parseFloat((e.target as HTMLInputElement).value))"
        @dblclick="mixer.setCueMix(0)"
        title="CUE/MIX: blend cue signal with master output in headphones"
      />
      <span class="topstrip__label topstrip__label--dim">MIX</span>
      <select
        class="topstrip__select"
        :value="mixer.cueDeviceId"
        @change="(e) => mixer.setCueOutputDevice((e.target as HTMLSelectElement).value, 0)"
      >
        <option value="">Not configured</option>
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
          Ch {{ offset + 1 }}-{{ offset + 2 }}
        </option>
      </select>

      <button class="topstrip__refresh" @click="mixer.loadOutputDevices()">↻</button>
      <span v-if="mixer.deviceError" class="topstrip__error">{{ mixer.deviceError }}</span>
    </template>

    <button class="topstrip__settings-btn" title="Settings (⌘,)" @click="emit('open-settings')">
      ⚙
    </button>
  </div>
</template>

<script setup lang="ts">
import { computed, ref, onMounted, onUnmounted } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { useMixerStore } from '@renderer/stores/mixer';
import { DECKS_DISPOSITION, type DeckId } from '@renderer/stores/decks';
import { useSettingsStore } from '@renderer/stores/settings';
import { vuParam, smoothParam, stepPeak, type PeakState } from '@renderer/utils/meter';

defineProps<{ editMode: boolean }>();
const emit = defineEmits<{ 'toggle-edit': []; 'open-settings': [] }>();

const mixer = useMixerStore();
const settings = useSettingsStore();

const activeDecks = computed<DeckId[]>(() =>
  mixer.deckCount === 2 ? ['A', 'B'] : [...DECKS_DISPOSITION]
);

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
  const [l, r] = await invoke<[number, number]>('get_master_level');
  const newParamL = vuParam(l);
  const newParamR = vuParam(r);
  paramL.value = smoothParam(paramL.value, newParamL);
  paramR.value = smoothParam(paramR.value, newParamR);
  peakL.value = stepPeak(peakL.value, newParamL);
  peakR.value = stepPeak(peakR.value, newParamR);
  rafId = requestAnimationFrame(pollLevels);
}

const isRecording = ref(false);

async function onRecClick() {
  if (isRecording.value) {
    isRecording.value = false;
    const tempPath = await invoke<string>('stop_recording');
    const destPath = await invoke<string | null>('pick_save_path', {
      format: settings.recordingFormat
    });
    if (destPath) {
      await invoke('save_recording', { src: tempPath, dest: destPath });
    } else {
      await invoke('discard_recording', { path: tempPath });
    }
  } else {
    await invoke('start_recording', {
      bitDepth: settings.recordingBitDepth,
      useFlac: settings.recordingFormat === 'flac'
    });
    isRecording.value = true;
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
  gap: 6px;
  padding: 0 12px;
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
    #ef4444 92%,
    #ef4444 100%
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
  letter-spacing: 0.12em;
  padding: 2px 8px;
  border-radius: 3px;
  cursor: not-allowed;
  user-select: none;
}

.topstrip__swarm-btn--active {
  border-color: #fbbf24;
  color: #fbbf24;
  background: color-mix(in srgb, #fbbf24 15%, transparent);
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
  font-size: 9px;
  font-weight: 700;
  letter-spacing: 0.1em;
  color: var(--color-muted);
  opacity: 0.5;
}

.topstrip__swarm-deck--on {
  color: #fbbf24;
  opacity: 1;
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
  letter-spacing: 0.1em;
  font-size: 9px;
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
  background: transparent;
  border: 1px solid var(--color-border);
  color: var(--color-muted);
  font-family: var(--font);
  font-size: 10px;
  letter-spacing: 0.12em;
  padding: 2px 8px;
  border-radius: 3px;
  cursor: pointer;
}

.topstrip__rec-btn:hover {
  border-color: #e55;
  color: #e55;
}

.topstrip__rec-btn--active {
  border-color: #e55;
  color: #e55;
  background: color-mix(in srgb, #e55 15%, transparent);
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

.topstrip__edit-btn {
  background: transparent;
  border: 1px solid var(--color-border);
  color: var(--color-muted);
  font-family: var(--font);
  font-size: 10px;
  letter-spacing: 0.12em;
  padding: 2px 8px;
  border-radius: 3px;
  cursor: pointer;
}

.topstrip__edit-btn:hover {
  border-color: #a855f7;
  color: #a855f7;
}

.topstrip__edit-btn--active {
  border-color: #a855f7;
  color: #a855f7;
  background: color-mix(in srgb, #a855f7 15%, transparent);
}

.topstrip__deck-count-btn {
  background: transparent;
  border: 1px solid var(--color-border);
  color: var(--color-muted);
  font-family: var(--font);
  font-size: 10px;
  letter-spacing: 0.12em;
  padding: 2px 8px;
  border-radius: 3px;
  cursor: pointer;
}

.topstrip__deck-count-btn:hover {
  border-color: var(--color-text);
  color: var(--color-text);
}

.topstrip__refresh {
  background: transparent;
  border: 1px solid var(--color-border);
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
  color: #e55;
  font-size: 10px;
}

.topstrip__settings-btn {
  background: transparent;
  border: 1px solid var(--color-border);
  color: var(--color-muted);
  font-size: 13px;
  width: 22px;
  height: 22px;
  border-radius: 3px;
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 0;
  flex-shrink: 0;
  transition:
    border-color 0.1s,
    color 0.1s;
}

.topstrip__settings-btn:hover {
  border-color: var(--color-text);
  color: var(--color-text);
}
</style>
