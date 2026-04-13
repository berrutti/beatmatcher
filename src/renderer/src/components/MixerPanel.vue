<template>
  <div class="mixer">
    <div class="mixer__channels">
      <div class="mixer__channel">
        <span class="mixer__channel-label" :style="{ color: store.deckA.accent }">A</span>
        <input
          type="range"
          class="mixer__fader"
          min="0"
          max="1"
          step="0.01"
          :value="store.volume.A"
          orient="vertical"
          :style="{ '--fader-accent': store.deckA.accent }"
          @input="(e) => store.setVolume('A', parseFloat((e.target as HTMLInputElement).value))"
        />
        <button
          class="mixer__cue-btn"
          :class="{ 'mixer__cue-btn--active': store.cueActive.A }"
          @click="store.setCueActive('A', !store.cueActive.A)"
        >
          CUE
        </button>
      </div>

      <div class="mixer__channel">
        <span class="mixer__channel-label" :style="{ color: store.deckB.accent }">B</span>
        <input
          type="range"
          class="mixer__fader"
          min="0"
          max="1"
          step="0.01"
          :value="store.volume.B"
          orient="vertical"
          :style="{ '--fader-accent': store.deckB.accent }"
          @input="(e) => store.setVolume('B', parseFloat((e.target as HTMLInputElement).value))"
        />
        <button
          class="mixer__cue-btn"
          :class="{ 'mixer__cue-btn--active': store.cueActive.B }"
          @click="store.setCueActive('B', !store.cueActive.B)"
        >
          CUE
        </button>
      </div>
    </div>

    <div v-if="store.devicesLoaded" class="mixer__devices">
      <div class="mixer__device-row">
        <span class="mixer__device-label">MASTER OUT</span>
        <select
          class="mixer__device-select"
          :value="store.mainDeviceId"
          @change="(e) => store.setMainOutputDevice((e.target as HTMLSelectElement).value, 0)"
        >
          <option v-for="d in store.outputDevices" :key="d.id" :value="d.id">
            {{ d.name }}
          </option>
        </select>
        <select
          v-if="mainDevice && mainDevice.channels > 2"
          class="mixer__device-select mixer__device-select--channels"
          :value="store.mainChannelOffset"
          @change="(e) => store.setMainOutputDevice(store.mainDeviceId, parseInt((e.target as HTMLSelectElement).value))"
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
          :value="store.cueDeviceId"
          @change="(e) => store.setCueOutputDevice((e.target as HTMLSelectElement).value, 0)"
        >
          <option value="">Not configured</option>
          <option v-for="d in store.outputDevices" :key="d.id" :value="d.id">
            {{ d.name }}
          </option>
        </select>
        <select
          v-if="cueDevice && cueDevice.channels > 2"
          class="mixer__device-select mixer__device-select--channels"
          :value="store.cueChannelOffset"
          @change="(e) => store.setCueOutputDevice(store.cueDeviceId, parseInt((e.target as HTMLSelectElement).value))"
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
      <button class="mixer__refresh-btn" @click="store.loadOutputDevices()">
        REFRESH DEVICES
      </button>
      <p v-if="store.deviceError" class="mixer__error">{{ store.deviceError }}</p>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted } from 'vue';
import { useDecksStore } from '@renderer/stores/decks';

const store = useDecksStore();

const mainDevice = computed(() => store.outputDevices.find((d) => d.id === store.mainDeviceId) ?? null);
const cueDevice = computed(() => store.outputDevices.find((d) => d.id === store.cueDeviceId) ?? null);

function channelPairs(totalChannels: number): number[] {
  const offsets: number[] = [];
  for (let i = 0; i + 1 < totalChannels; i += 2) {
    offsets.push(i);
  }
  return offsets;
}

onMounted(() => {
  store.loadOutputDevices();
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
