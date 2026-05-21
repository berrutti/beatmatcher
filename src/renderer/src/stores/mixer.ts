import { defineStore } from 'pinia';
import { reactive, ref } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import type { DeckId } from './decks';
import { storageGet, storageSet, STORAGE_KEYS } from '@renderer/utils/storage';
import { useSettingsStore } from '@renderer/stores/settings';

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
    E: false // can never be active
  });
  const filter = reactive<Record<DeckId, number>>({ A: 0, B: 0, C: 0, D: 0, E: 0 });
  const filterEnabled = reactive<Record<DeckId, boolean>>({
    A: false,
    B: false,
    C: false,
    D: false,
    E: false // can never be active
  });

  const storedCount = storageGet<number>(STORAGE_KEYS.deckCount, 4);
  const deckCount = ref<2 | 4>(storedCount === 2 ? 2 : 4);

  function toggleDeckCount() {
    deckCount.value = deckCount.value === 4 ? 2 : 4;
    storageSet(STORAGE_KEYS.deckCount, deckCount.value);
  }

  function setDeckCount(count: 2 | 4) {
    deckCount.value = count;
    storageSet(STORAGE_KEYS.deckCount, count);
  }

  const masterGain = ref(DEFAULT_MASTER_GAIN);

  function setMasterGain(gain: number) {
    masterGain.value = Math.max(0, Math.min(1, gain));
    invoke('set_master_gain', { gain: masterGain.value });
  }

  const cueMix = ref(0);

  function setCueMix(mix: number) {
    cueMix.value = Math.max(0, Math.min(1, mix));
    invoke('set_cue_mix', { mix: cueMix.value });
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
    invoke('set_filter', { deck: deckId, value: filter[deckId] });
  }

  function toggleFilter(deckId: DeckId) {
    filterEnabled[deckId] = !filterEnabled[deckId];
    invoke('set_filter_active', { deck: deckId, active: filterEnabled[deckId] });
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

  function alternateChannelOffset(
    totalChannels: number,
    avoidOffset: number,
    preferred: number
  ): number | null {
    if (preferred !== avoidOffset) return preferred;
    for (let i = 0; i + 1 < totalChannels; i += 2) {
      if (i !== avoidOffset) return i;
    }
    return null;
  }

  async function setMainOutputDevice(deviceId: string, channelOffset?: number): Promise<void> {
    deviceError.value = '';
    const newMainOffset = channelOffset ?? mainChannelOffset.value;
    if (deviceId && deviceId === cueDeviceId.value) {
      const device = outputDevices.value.find((d) => d.id === deviceId);
      if (device && device.channels > 2) {
        const newCueOffset = alternateChannelOffset(
          device.channels,
          newMainOffset,
          cueChannelOffset.value
        );
        if (newCueOffset !== null) {
          cueChannelOffset.value = newCueOffset;
          try {
            await invoke('set_cue_device', { deviceId, channelOffset: newCueOffset });
          } catch {
            /* best-effort */
          }
        }
      } else {
        cueDeviceId.value = '';
        try {
          await invoke('set_cue_device', { deviceId: '', channelOffset: cueChannelOffset.value });
        } catch {
          /* best-effort */
        }
      }
    }
    mainDeviceId.value = deviceId;
    if (channelOffset !== undefined) mainChannelOffset.value = channelOffset;
    try {
      await invoke('set_main_device', { deviceId, channelOffset: mainChannelOffset.value });
    } catch (e) {
      deviceError.value = `Master out: ${e}`;
    }
  }

  async function getDeckLevels(): Promise<Record<string, [number, number]>> {
    return invoke<Record<string, [number, number]>>('get_deck_levels');
  }

  async function getMasterLevel(): Promise<[number, number]> {
    return invoke<[number, number]>('get_master_level');
  }

  async function startRecording(): Promise<void> {
    const settings = useSettingsStore();
    await invoke('start_recording', {
      bitDepth: settings.recordingBitDepth,
      useFlac: settings.recordingFormat === 'flac',
      recordSession: settings.recordSession
    });
  }

  async function stopRecording(): Promise<string> {
    return invoke<string>('stop_recording');
  }

  async function pickSavePath(): Promise<string | null> {
    const settings = useSettingsStore();
    return invoke<string | null>('pick_save_path', { format: settings.recordingFormat });
  }

  async function saveRecording(src: string, dest: string): Promise<void> {
    await invoke('save_recording', { src, dest });
  }

  async function discardRecording(path: string): Promise<void> {
    await invoke('discard_recording', { path });
  }

  async function setCueOutputDevice(deviceId: string, channelOffset?: number): Promise<void> {
    deviceError.value = '';
    const newCueOffset = channelOffset ?? cueChannelOffset.value;
    if (deviceId && deviceId === mainDeviceId.value) {
      const device = outputDevices.value.find((d) => d.id === deviceId);
      if (device && device.channels > 2) {
        const newMainOffset = alternateChannelOffset(
          device.channels,
          newCueOffset,
          mainChannelOffset.value
        );
        if (newMainOffset !== null) {
          mainChannelOffset.value = newMainOffset;
          try {
            await invoke('set_main_device', { deviceId, channelOffset: newMainOffset });
          } catch {
            /* best-effort */
          }
        }
      } else {
        mainDeviceId.value = '';
        try {
          await invoke('set_main_device', { deviceId: '', channelOffset: mainChannelOffset.value });
        } catch {
          /* best-effort */
        }
      }
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
    cueActive,
    cueChannelOffset,
    cueDeviceId,
    cueMix,
    deckCount,
    deviceError,
    devicesLoaded,
    filter,
    filterEnabled,
    mainChannelOffset,
    mainDeviceId,
    masterGain,
    outputDevices,
    swarmMode,
    swarmSelected,
    volume,
    discardRecording,
    getDeckLevels,
    getMasterLevel,
    loadOutputDevices,
    pickSavePath,
    saveRecording,
    setCueActive,
    setCueMix,
    setCueOutputDevice,
    setDeckCount,
    setFilter,
    setMainOutputDevice,
    setMasterGain,
    setSwarmChannel,
    setSwarmMode,
    setVolume,
    startRecording,
    stopRecording,
    toggleDeckCount,
    toggleFilter
  };
});
