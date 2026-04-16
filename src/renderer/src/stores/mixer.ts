import { defineStore } from 'pinia';
import { reactive, ref } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import type { DeckId } from './decks';

type DeviceInfo = { id: string; name: string; isDefault: boolean; channels: number };

export const useMixerStore = defineStore('mixer', () => {
  const outputDevices = ref<DeviceInfo[]>([]);
  const devicesLoaded = ref(false);
  const mainDeviceId = ref('');
  const cueDeviceId = ref('');
  const mainChannelOffset = ref(0);
  const cueChannelOffset = ref(0);
  const deviceError = ref('');

  const volume = reactive<Record<DeckId, number>>({ A: 1, B: 1 });
  const cueActive = reactive<Record<DeckId, boolean>>({ A: false, B: false });

  function setVolume(deckId: DeckId, v: number) {
    volume[deckId] = Math.max(0, Math.min(1, v));
    invoke('set_volume', { deck: deckId, gain: volume[deckId] });
  }

  function setCueActive(deckId: DeckId, active: boolean) {
    cueActive[deckId] = active;
    invoke('set_cue_active', { deck: deckId, active });
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
    volume,
    cueActive,
    setVolume,
    setCueActive,
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
