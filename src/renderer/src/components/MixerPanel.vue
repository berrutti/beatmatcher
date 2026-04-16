<template>
  <div class="mixer">
    <div class="mixer__channels">
      <div class="mixer__channel">
        <span class="mixer__channel-label" :style="{ color: decks.deckA.accent }">A</span>
        <input
          type="range"
          class="mixer__fader"
          min="0"
          max="1"
          step="0.01"
          :value="mixer.volume.A"
          orient="vertical"
          :style="{ '--fader-accent': decks.deckA.accent }"
          @input="(e) => mixer.setVolume('A', parseFloat((e.target as HTMLInputElement).value))"
        />
        <button
          class="mixer__cue-btn"
          :class="{ 'mixer__cue-btn--active': mixer.cueActive.A }"
          @click="mixer.setCueActive('A', !mixer.cueActive.A)"
        >
          CUE
        </button>
      </div>

      <div class="mixer__channel">
        <span class="mixer__channel-label" :style="{ color: decks.deckB.accent }">B</span>
        <input
          type="range"
          class="mixer__fader"
          min="0"
          max="1"
          step="0.01"
          :value="mixer.volume.B"
          orient="vertical"
          :style="{ '--fader-accent': decks.deckB.accent }"
          @input="(e) => mixer.setVolume('B', parseFloat((e.target as HTMLInputElement).value))"
        />
        <button
          class="mixer__cue-btn"
          :class="{ 'mixer__cue-btn--active': mixer.cueActive.B }"
          @click="mixer.setCueActive('B', !mixer.cueActive.B)"
        >
          CUE
        </button>
      </div>
    </div>

    <div v-if="mixer.devicesLoaded" class="mixer__devices">
      <div class="mixer__device-row">
        <span class="mixer__device-label">MASTER OUT</span>
        <select
          class="mixer__device-select"
          :value="mixer.mainDeviceId"
          @change="(e) => mixer.setMainOutputDevice((e.target as HTMLSelectElement).value, 0)"
        >
          <option v-for="d in mixer.outputDevices" :key="d.id" :value="d.id">
            {{ d.name }}
          </option>
        </select>
        <select
          v-if="mainDevice && mainDevice.channels > 2"
          class="mixer__device-select mixer__device-select--channels"
          :value="mixer.mainChannelOffset"
          @change="(e) => mixer.setMainOutputDevice(mixer.mainDeviceId, parseInt((e.target as HTMLSelectElement).value))"
        >
          <option
            v-for="offset in channelPairs(mainDevice.channels)"
            :key="offset"
            :value="offset"
          >
            Ch {{ offset + 1 }}-{{ offset + 2 }}
          </option>
        </select>
      </div>
      <div class="mixer__device-row">
        <span class="mixer__device-label">CUE OUT</span>
        <select
          class="mixer__device-select"
          :value="mixer.cueDeviceId"
          @change="(e) => mixer.setCueOutputDevice((e.target as HTMLSelectElement).value, 0)"
        >
          <option value="">Not configured</option>
          <option v-for="d in mixer.outputDevices" :key="d.id" :value="d.id">
            {{ d.name }}
          </option>
        </select>
        <select
          v-if="cueDevice && cueDevice.channels > 2"
          class="mixer__device-select mixer__device-select--channels"
          :value="mixer.cueChannelOffset"
          @change="(e) => mixer.setCueOutputDevice(mixer.cueDeviceId, parseInt((e.target as HTMLSelectElement).value))"
        >
          <option
            v-for="offset in channelPairs(cueDevice.channels)"
            :key="offset"
            :value="offset"
          >
            Ch {{ offset + 1 }}-{{ offset + 2 }}
          </option>
        </select>
      </div>
      <button class="mixer__refresh-btn" @click="mixer.loadOutputDevices()">
        REFRESH DEVICES
      </button>
      <p v-if="mixer.deviceError" class="mixer__error">{{ mixer.deviceError }}</p>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted } from 'vue';
import { useDecksStore } from '@renderer/stores/decks';
import { useMixerStore } from '@renderer/stores/mixer';

const decks = useDecksStore();
const mixer = useMixerStore();

const mainDevice = computed(() => mixer.outputDevices.find((d) => d.id === mixer.mainDeviceId) ?? null);
const cueDevice = computed(() => mixer.outputDevices.find((d) => d.id === mixer.cueDeviceId) ?? null);

function channelPairs(totalChannels: number): number[] {
  const offsets: number[] = [];
  for (let i = 0; i + 1 < totalChannels; i += 2) {
    offsets.push(i);
  }
  return offsets;
}

onMounted(() => {
  mixer.loadOutputDevices();
});
</script>

<style scoped>
.mixer {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 0.6em;
  padding: 0.8em 0.5em;
  border-top: 1px solid var(--color-border);
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

.mixer__devices {
  display: flex;
  flex-direction: column;
  gap: 0.4em;
  width: 100%;
  padding: 0 0.5em;
}

.mixer__device-row {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 0.3em;
  width: 100%;
}

.mixer__device-label {
  font-size: 0.55em;
  letter-spacing: 0.15em;
  color: var(--color-muted);
}

.mixer__device-select {
  background: var(--color-surface);
  border: 1px solid var(--color-border);
  color: var(--color-text);
  font-family: var(--font);
  font-size: 0.6em;
  padding: 0.3em 0.5em;
  border-radius: 3px;
  cursor: pointer;
  width: 100%;
  max-width: 14em;
}
.mixer__device-select--channels {
  max-width: 7em;
}
.mixer__device-select:focus {
  outline: none;
  border-color: #555;
}

.mixer__refresh-btn {
  background: transparent;
  border: 1px solid var(--color-border);
  color: var(--color-muted);
  font-family: var(--font);
  font-size: 0.5em;
  letter-spacing: 0.15em;
  padding: 0.3em 0.7em;
  border-radius: 3px;
  cursor: pointer;
  align-self: center;
  transition: border-color 0.1s, color 0.1s;
}
.mixer__refresh-btn:hover {
  border-color: var(--color-text);
  color: var(--color-text);
}

.mixer__error {
  font-size: 0.5em;
  color: #e55;
  text-align: center;
  margin: 0;
  padding: 0 0.5em;
}
</style>
