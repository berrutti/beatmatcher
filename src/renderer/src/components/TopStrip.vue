<template>
  <div class="topstrip">
    <div class="topstrip__meters">
      <span class="topstrip__meter-label">L</span>
      <div class="topstrip__meter"><div class="topstrip__meter-fill" /></div>
      <span class="topstrip__meter-label">R</span>
      <div class="topstrip__meter"><div class="topstrip__meter-fill topstrip__meter-fill--r" /></div>
    </div>

    <button
      class="topstrip__edit-btn"
      :class="{ 'topstrip__edit-btn--active': editMode }"
      @click="emit('toggle-edit')"
    >EDIT</button>

    <div class="topstrip__spacer" />

    <template v-if="mixer.devicesLoaded">
      <span class="topstrip__label">MASTER</span>
      <select
        class="topstrip__select"
        :value="mixer.mainDeviceId"
        @change="(e) => mixer.setMainOutputDevice((e.target as HTMLSelectElement).value, 0)"
      >
        <option v-for="d in mixer.outputDevices" :key="d.id" :value="d.id">{{ d.name }}</option>
      </select>
      <select
        v-if="mainDevice && mainDevice.channels > 2"
        class="topstrip__select topstrip__select--ch"
        :value="mixer.mainChannelOffset"
        @change="(e) => mixer.setMainOutputDevice(mixer.mainDeviceId, parseInt((e.target as HTMLSelectElement).value))"
      >
        <option v-for="offset in channelPairs(mainDevice.channels)" :key="offset" :value="offset">
          Ch {{ offset + 1 }}-{{ offset + 2 }}
        </option>
      </select>

      <span class="topstrip__label">CUE</span>
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
        @change="(e) => mixer.setCueOutputDevice(mixer.cueDeviceId, parseInt((e.target as HTMLSelectElement).value))"
      >
        <option v-for="offset in channelPairs(cueDevice.channels)" :key="offset" :value="offset">
          Ch {{ offset + 1 }}-{{ offset + 2 }}
        </option>
      </select>

      <button class="topstrip__refresh" @click="mixer.loadOutputDevices()">↻</button>
      <span v-if="mixer.deviceError" class="topstrip__error">{{ mixer.deviceError }}</span>
    </template>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted } from 'vue';
import { useMixerStore } from '@renderer/stores/mixer';

defineProps<{ editMode: boolean }>();
const emit = defineEmits<{ 'toggle-edit': [] }>();

const mixer = useMixerStore();

const mainDevice = computed(() => mixer.outputDevices.find((d) => d.id === mixer.mainDeviceId) ?? null);
const cueDevice = computed(() => mixer.outputDevices.find((d) => d.id === mixer.cueDeviceId) ?? null);

function channelPairs(totalChannels: number): number[] {
  const offsets: number[] = [];
  for (let i = 0; i + 1 < totalChannels; i += 2) offsets.push(i);
  return offsets;
}

onMounted(() => {
  mixer.loadOutputDevices();
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
  background: var(--color-surface);
  border: 0.5px solid var(--color-border);
  border-radius: 1px;
  overflow: hidden;
}

.topstrip__meter-fill {
  height: 100%;
  width: 55%;
  background: var(--color-muted);
  opacity: 0.5;
}

.topstrip__meter-fill--r {
  width: 48%;
}

.topstrip__spacer {
  flex: 1;
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

.topstrip__refresh {
  background: transparent;
  border: 1px solid var(--color-border);
  color: var(--color-muted);
  font-family: var(--font);
  font-size: 12px;
  padding: 1px 6px;
  border-radius: 3px;
  cursor: pointer;
  transition: border-color 0.1s, color 0.1s;
}

.topstrip__refresh:hover {
  border-color: var(--color-text);
  color: var(--color-text);
}

.topstrip__error {
  color: #e55;
  font-size: 10px;
}
</style>
