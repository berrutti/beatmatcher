import { defineStore } from 'pinia';
import { reactive, ref } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import type { DeckId } from './decks';

type DeviceInfo = { id: string; name: string; isDefault: boolean; channels: number };

// -2 dBFS: must match DEFAULT_MASTER_GAIN in audio.rs
const DEFAULT_MASTER_GAIN = 0.7943;

export const useMixerStore = defineStore('mixer', () => {
  const outputDevices = ref<DeviceInfo[]>([]);
  const devicesLoaded = ref(false);
  const mainDeviceId = ref('');
  const cueDeviceId = ref('');
  const mainChannelOffset = ref(0);
  const cueChannelOffset = ref(0);
  const deviceError = ref('');

  const volume = reactive<Record<DeckId, number>>({ A: 1, B: 1, C: 1, D: 1, E: 1 });
  const cueActive = reactive<Record<DeckId, boolean>>({
    A: false,
    B: false,
    C: false,
    D: false,
    E: false
  });
  const filter = reactive<Record<DeckId, number>>({ A: 0, B: 0, C: 0, D: 0, E: 0 });
  const filterEnabled = reactive<Record<DeckId, boolean>>({
    A: false,
    B: false,
    C: false,
    D: false,
    E: false
  });

  const masterGain = ref(DEFAULT_MASTER_GAIN);

  function setMasterGain(gain: number) {
    masterGain.value = Math.max(0, Math.min(1, gain));
    invoke('set_master_gain', { gain: masterGain.value });
  }

  const swarmMode = ref(false);
  const swarmSelected = reactive<Record<DeckId, boolean>>({
    A: false,
    B: false,
    C: false,
    D: false,
    E: false
  });

  function setSwarmMode(active: boolean) {
    swarmMode.value = active;
    if (!active) {
      (Object.keys(swarmSelected) as DeckId[]).forEach((k) => {
        swarmSelected[k] = false;
      });
    }
  }

  function setSwarmChannel(deckId: DeckId, active: boolean) {
    swarmSelected[deckId] = active;
  }

  function setVolume(deckId: DeckId, v: number) {
    volume[deckId] = Math.max(0, Math.min(1, v));
    invoke('set_volume', { deck: deckId, gain: volume[deckId] });
  }

  function setCueActive(deckId: DeckId, active: boolean) {
    cueActive[deckId] = active;
    invoke('set_cue_active', { deck: deckId, active });
  }

  function setFilter(deckId: DeckId, v: number) {
    filter[deckId] = Math.max(-1, Math.min(1, v));
    invoke('set_filter', { deck: deckId, value: filterEnabled[deckId] ? filter[deckId] : 0 });
  }

  function toggleFilter(deckId: DeckId) {
    filterEnabled[deckId] = !filterEnabled[deckId];
    invoke('set_filter', { deck: deckId, value: filterEnabled[deckId] ? filter[deckId] : 0 });
  }

  async function loadOutputDevices(): Promise<void> {
    deviceError.value = '';
    outputDevices.value = await invoke<DeviceInfo[]>('list_audio_devices');
    devicesLoaded.value = true;
    if (!mainDeviceId.value) {
      const defaultDevice = outputDevices.value.find((d) => d.isDefault);
      if (defaultDevice) mainDeviceId.value = defaultDevice.id;
    }
  }

  async function setMainOutputDevice(deviceId: string, channelOffset?: number): Promise<void> {
    deviceError.value = '';
    if (deviceId && deviceId === cueDeviceId.value) {
      cueDeviceId.value = '';
      await invoke('set_cue_device', { deviceId: '', channelOffset: cueChannelOffset.value }).catch(
        () => {}
      );
    }
    mainDeviceId.value = deviceId;
    if (channelOffset !== undefined) mainChannelOffset.value = channelOffset;
    try {
      await invoke('set_main_device', { deviceId, channelOffset: mainChannelOffset.value });
    } catch (e) {
      deviceError.value = `Master out: ${e}`;
    }
  }

  async function setCueOutputDevice(deviceId: string, channelOffset?: number): Promise<void> {
    deviceError.value = '';
    if (deviceId && deviceId === mainDeviceId.value) {
      mainDeviceId.value = '';
      await invoke('set_main_device', {
        deviceId: '',
        channelOffset: mainChannelOffset.value
      }).catch(() => {});
    }
    cueDeviceId.value = deviceId;
    if (channelOffset !== undefined) cueChannelOffset.value = channelOffset;
    if (!deviceId) return;
    try {
      await invoke('set_cue_device', { deviceId, channelOffset: cueChannelOffset.value });
    } catch (e) {
      deviceError.value = `Cue out: ${e}`;
    }
  }

  return {
    masterGain,
    setMasterGain,
    volume,
    cueActive,
    filter,
    filterEnabled,
    swarmMode,
    swarmSelected,
    setSwarmMode,
    setSwarmChannel,
    setVolume,
    setCueActive,
    setFilter,
    toggleFilter,
    outputDevices,
    devicesLoaded,
    mainDeviceId,
    cueDeviceId,
    mainChannelOffset,
    cueChannelOffset,
    deviceError,
    loadOutputDevices,
    setMainOutputDevice,
    setCueOutputDevice
  };
});
