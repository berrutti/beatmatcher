import { defineStore } from 'pinia';
import { computed, reactive, ref } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { DECKS_DISPOSITION, type DeckId } from './decks';
import { storageGet, storageSet, STORAGE_KEYS } from '@renderer/utils/storage';
import { useSettingsStore } from '@renderer/stores/settings';

type DeviceInfo = { id: string; name: string; isDefault: boolean; channels: number };
type EqBand = 'low' | 'mid' | 'high';
type EqState = { low: number; mid: number; high: number };

// Defined in session-core; copies exist here because WASM is not initialized
// at module-evaluation time. Pinned by the editConstants parity test.
export const DEFAULT_MASTER_GAIN = 0.7943;
export const EQ_MIN_DB = -26;
export const EQ_MAX_DB = 6;
export const FILTER_DEAD_ZONE = 0.05;

const LIVE_DECKS: DeckId[] = ['A', 'B', 'C', 'D'];

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
  const eq = reactive<Record<DeckId, EqState>>({
    A: { low: 0, mid: 0, high: 0 },
    B: { low: 0, mid: 0, high: 0 },
    C: { low: 0, mid: 0, high: 0 },
    D: { low: 0, mid: 0, high: 0 },
    E: { low: 0, mid: 0, high: 0 }
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

  const activeDecks = computed<DeckId[]>(() =>
    deckCount.value === 2 ? ['A', 'B'] : [...DECKS_DISPOSITION]
  );

  const showWaveformStrip = ref(storageGet<boolean>(STORAGE_KEYS.showWaveformStrip, true));

  function toggleWaveformStrip() {
    showWaveformStrip.value = !showWaveformStrip.value;
    storageSet(STORAGE_KEYS.showWaveformStrip, showWaveformStrip.value);
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

  function setEq(deckId: DeckId, band: EqBand, db: number) {
    eq[deckId][band] = Math.max(EQ_MIN_DB, Math.min(EQ_MAX_DB, db));
    invoke('set_eq', { deck: deckId, band, db: eq[deckId][band] });
  }

  function reset(): void {
    for (const deckId of LIVE_DECKS) {
      setVolume(deckId, 1);
      setEq(deckId, 'low', 0);
      setEq(deckId, 'mid', 0);
      setEq(deckId, 'high', 0);
      setFilter(deckId, 0);
      filterEnabled[deckId] = false;
      invoke('set_filter_active', { deck: deckId, active: false });
      cueActive[deckId] = false;
      invoke('set_cue_active', { deck: deckId, active: false });
    }
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

  const isRecording = ref(false);
  const playedPaths = reactive(new Set<string>());

  function markPlayed(path: string): void {
    playedPaths.add(path);
  }

  async function startRecording(): Promise<void> {
    isRecording.value = true;
    const settings = useSettingsStore();
    const fmt = settings.recordingFormat;
    await invoke('start_recording', {
      bitDepth: fmt === 'wav-16' ? 16 : 32,
      useFlac: fmt === 'flac',
      // The cue sheet is derived from the event log, so writing one requires
      // capturing the session even when no .bms is kept.
      recordSession: fmt === 'session' || settings.recordBms || settings.recordCue
    });
  }

  async function stopRecording(): Promise<string> {
    const tempPath = await invoke<string>('stop_recording');
    isRecording.value = false;
    return tempPath;
  }

  async function pickSavePath(): Promise<string | null> {
    const settings = useSettingsStore();
    const fmt = settings.recordingFormat;
    const dialogFormat = fmt === 'flac' ? 'flac' : fmt === 'session' ? 'session' : 'wav';
    return invoke<string | null>('pick_save_path', { format: dialogFormat });
  }

  async function saveRecording(src: string, dest: string): Promise<void> {
    const settings = useSettingsStore();
    if (settings.recordingFormat === 'session') {
      await invoke('save_bms_only', { src, dest });
    } else {
      await invoke('save_recording', {
        src,
        dest,
        writeBms: settings.recordBms,
        writeCue: settings.recordCue
      });
    }
  }

  async function discardRecording(path: string): Promise<void> {
    await invoke('discard_recording', { path });
  }

  async function renderSession(
    sessionPath: string,
    outputPath: string,
    useFlac: boolean
  ): Promise<void> {
    await invoke('render_session_to_file', {
      sessionPath,
      outputPath,
      useFlac,
      writeCue: useSettingsStore().recordCue
    });
  }

  async function pickRenderOutputPath(useFlac: boolean): Promise<string | null> {
    return invoke<string | null>('pick_save_path', { format: useFlac ? 'flac' : 'wav' });
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
    activeDecks,
    cueActive,
    cueChannelOffset,
    cueDeviceId,
    cueMix,
    deckCount,
    deviceError,
    devicesLoaded,
    eq,
    filter,
    filterEnabled,
    mainChannelOffset,
    mainDeviceId,
    masterGain,
    outputDevices,
    showWaveformStrip,
    swarmMode,
    swarmSelected,
    volume,
    isRecording,
    playedPaths,
    markPlayed,
    discardRecording,
    getDeckLevels,
    getMasterLevel,
    loadOutputDevices,
    pickRenderOutputPath,
    pickSavePath,
    renderSession,
    reset,
    saveRecording,
    setCueActive,
    setCueMix,
    setCueOutputDevice,
    setDeckCount,
    setEq,
    setFilter,
    setMainOutputDevice,
    setMasterGain,
    setSwarmChannel,
    setSwarmMode,
    setVolume,
    startRecording,
    stopRecording,
    toggleDeckCount,
    toggleFilter,
    toggleWaveformStrip
  };
});
