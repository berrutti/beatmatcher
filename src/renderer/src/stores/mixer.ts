import { defineStore } from 'pinia';
import { computed, reactive, ref } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { DECKS_DISPOSITION } from './decks';
import type { DeckId } from '@renderer/utils/types';
import { storageGet, storageSet, STORAGE_KEYS } from '@renderer/utils/storage';
import { useSettingsStore, LIVE_MIXER_ID } from '@renderer/stores/settings';
import { editConstants, mixerParams, type MixerParamSpec } from '@renderer/utils/sessionCore';

type DeviceInfo = { id: string; name: string; isDefault: boolean; channels: number };
type ParamChange = { deck: string; slot: string; param: string; value: number };
export type EqBand = 'low' | 'mid' | 'high';
export type XfaderAssign = 'thru' | 'a' | 'b';
export type XfaderSide = 'a' | 'b';
type EqState = { low: number; mid: number; high: number };
type EqBandSpec = MixerParamSpec & { param: EqBand };
export type SwarmTarget = { slot: 'volume' } | { slot: 'filter' } | { slot: 'eq'; band: EqBand };

const LIVE_DECKS: DeckId[] = ['A', 'B', 'C', 'D'];

export const useMixerStore = defineStore('mixer', () => {
  // Store setup runs on first use, which is after the app's async init().
  const { defaultMasterGain } = editConstants();

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
  const EQ_BANDS: EqBand[] = ['low', 'mid', 'high'];

  // Centre, and every deck through, so the crossfader is inert until a deck is
  // deliberately put on a side. Matches the engine's own default.
  const xfaderPosition = ref(0);
  const xfaderAssign = reactive<Record<DeckId, XfaderAssign>>({
    A: 'thru',
    B: 'thru',
    C: 'thru',
    D: 'thru',
    E: 'thru' // the edit deck never reaches the live mixer
  });

  // The live engine builds every strip on this one manifest. Ranges, steps and
  // defaults come from its descriptors rather than being restated here.
  const deckParams = mixerParams(LIVE_MIXER_ID);

  const eq = reactive<Record<DeckId, EqState>>({
    A: defaultEqState(),
    B: defaultEqState(),
    C: defaultEqState(),
    D: defaultEqState(),
    E: defaultEqState()
  });

  function defaultEqState(): EqState {
    return { low: eqDefault('low'), mid: eqDefault('mid'), high: eqDefault('high') };
  }

  const eqSpecs: EqBandSpec[] = EQ_BANDS.flatMap((band) => {
    const spec = deckParams[`eq/${band}`];
    return spec ? [{ ...spec, param: band }] : [];
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

  const masterGain = ref(defaultMasterGain);

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

  // Per deck, because scrubs overlap: two decks scrubbed at once, or a scrub whose
  // end is lost to a window blur, used to restore one deck's volume onto another
  // and leave the second silent.
  const scrubSavedVolume: Partial<Record<DeckId, number>> = {};

  function startScrubMute(deckId: DeckId) {
    if (scrubSavedVolume[deckId] === undefined) scrubSavedVolume[deckId] = volume[deckId];
    setVolume(deckId, 0);
  }

  function endScrubMute(deckId: DeckId) {
    const saved = scrubSavedVolume[deckId];
    if (saved === undefined) return;
    delete scrubSavedVolume[deckId];
    setVolume(deckId, saved);
  }

  function setXfaderPosition(position: number) {
    xfaderPosition.value = Math.max(-1, Math.min(1, position));
    invoke('set_xfader_position', { position: xfaderPosition.value });
  }

  function setXfaderAssign(deckId: DeckId, assign: XfaderAssign) {
    xfaderAssign[deckId] = assign;
    invoke('set_xfader_assign', { deck: deckId, assign });
  }

  // The UI has no button for `thru`: the two sides are one exclusive pair, so
  // deselecting the lit one is what takes the deck off the crossfader.
  function toggleXfaderAssign(deckId: DeckId, side: XfaderSide) {
    setXfaderAssign(deckId, xfaderAssign[deckId] === side ? 'thru' : side);
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

  function setEq(deckId: DeckId, band: EqBand, value: number) {
    const spec = deckParams[`eq/${band}`];
    if (!spec) return;
    eq[deckId][band] = Math.max(spec.min, Math.min(spec.max, value));
    invoke('set_eq', { deck: deckId, band, db: eq[deckId][band] });
  }

  function eqDefault(band: EqBand): number {
    return deckParams[`eq/${band}`]?.defaultValue ?? 0;
  }

  function swarmAffected(deckId: DeckId): DeckId[] {
    const selected = activeDecks.value.filter((candidate) => swarmSelected[candidate]);
    if (!selected.includes(deckId)) selected.push(deckId);
    return selected;
  }

  function swarmValue(deckId: DeckId, target: SwarmTarget): number {
    if (target.slot === 'volume') return volume[deckId];
    if (target.slot === 'filter') return filter[deckId];
    return eq[deckId][target.band];
  }

  function swarmWrite(deckId: DeckId, target: SwarmTarget, value: number) {
    if (target.slot === 'volume') setVolume(deckId, value);
    else if (target.slot === 'filter') setFilter(deckId, value);
    else setEq(deckId, target.band, value);
  }

  // A drag carries the gesture's delta to every selected channel, so they keep
  // the offsets the DJ set between them instead of collapsing onto one value.
  function swarmAdjust(deckId: DeckId, target: SwarmTarget, value: number) {
    if (!swarmMode.value) {
      swarmWrite(deckId, target, value);
      return;
    }
    const delta = value - swarmValue(deckId, target);
    for (const affected of swarmAffected(deckId)) {
      swarmWrite(affected, target, swarmValue(affected, target) + delta);
    }
  }

  function swarmReset(deckId: DeckId, target: SwarmTarget, value: number) {
    const affected = swarmMode.value ? swarmAffected(deckId) : [deckId];
    for (const deck of affected) swarmWrite(deck, target, value);
  }

  function isDeckId(id: string): id is DeckId {
    return Object.prototype.hasOwnProperty.call(volume, id);
  }

  function isEqBand(param: string): param is EqBand {
    return EQ_BANDS.some((band) => band === param);
  }

  // Engine-originated only, and deliberately does not invoke back: Rust never
  // pushes a value the UI itself wrote, so anything arriving here is a move the
  // store has not already made.
  function assignFromValue(value: number): XfaderAssign {
    if (value === 1) return 'a';
    if (value === 2) return 'b';
    return 'thru';
  }

  function applyEngineParam(change: ParamChange): void {
    // Master scope, so it arrives with no deck and has to be read before the
    // guard below rejects it.
    if (change.slot === 'xfader' && change.param === 'position') {
      xfaderPosition.value = change.value;
      return;
    }
    if (!isDeckId(change.deck)) return;
    if (change.slot === 'xfader' && change.param === 'assign') {
      xfaderAssign[change.deck] = assignFromValue(change.value);
      return;
    }
    if (change.slot === 'eq' && isEqBand(change.param)) {
      eq[change.deck][change.param] = change.value;
      return;
    }
    if (change.slot === 'fader' && change.param === 'gain') {
      volume[change.deck] = change.value;
      return;
    }
    if (change.slot === 'filter' && change.param === 'value') {
      filter[change.deck] = change.value;
      return;
    }
    if (change.slot === 'filter' && change.param === 'active') {
      filterEnabled[change.deck] = change.value !== 0;
      return;
    }
    if (change.slot === 'cue' && change.param === 'active') {
      cueActive[change.deck] = change.value !== 0;
    }
  }

  listen<ParamChange[]>('engine-params', (event) => {
    event.payload.forEach(applyEngineParam);
  });

  function reset(): void {
    setXfaderPosition(0);
    for (const deckId of LIVE_DECKS) {
      setXfaderAssign(deckId, 'thru');
      setVolume(deckId, 1);
      for (const band of EQ_BANDS) setEq(deckId, band, eqDefault(band));
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
    startScrubMute,
    endScrubMute,
    activeDecks,
    eqDefault,
    eqSpecs,
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
    xfaderPosition,
    xfaderAssign,
    isRecording,
    playedPaths,
    markPlayed,
    applyEngineParam,
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
    swarmAdjust,
    swarmReset,
    setXfaderAssign,
    setXfaderPosition,
    toggleXfaderAssign,
    setVolume,
    startRecording,
    stopRecording,
    toggleDeckCount,
    toggleFilter,
    toggleWaveformStrip
  };
});
