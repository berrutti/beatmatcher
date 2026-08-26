import { defineStore } from 'pinia';
import { computed, reactive, ref, watch } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { call } from '@renderer/tauriCommands';
import { listen } from '@tauri-apps/api/event';
import { DECKS_DISPOSITION, TWO_DECK_DISPOSITION } from './decks';
import type { DeckId } from '@renderer/utils/types';
import { storageGet, storageSet, STORAGE_KEYS } from '@renderer/utils/storage';

const SAVE_POLL_MS = 200;
import { useSettingsStore, LIVE_MIXER_ID } from '@renderer/stores/settings';
import { editConstants, mixerParams, type MixerParamSpec } from '@renderer/utils/sessionCore';

type DeviceInfo = { id: string; name: string; isDefault: boolean; channels: number };
type ParamChange = { deck: string; slot: string; param: string; value: number };
export type XfaderAssign = 'thru' | 'a' | 'b';
export type XfaderSide = 'a' | 'b';

const LIVE_DECKS: readonly DeckId[] = DECKS_DISPOSITION;

// How `mixerParams` keys its specs, and how the store keys a deck's values, so a
// param the manifest gained is reachable without anything here naming it.
export function paramKey(slot: string, param: string): string {
  return `${slot}/${param}`;
}

export const FADER_GAIN = paramKey('fader', 'gain');
export const FILTER_VALUE = paramKey('filter', 'value');
export const FILTER_ACTIVE = paramKey('filter', 'active');

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

  const cueActive = reactive<Record<DeckId, boolean>>({
    A: false,
    B: false,
    C: false,
    D: false,
    E: false // can never be active
  });

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

  // Left to right on the strip. Where a control sits is a layout decision, so it is stated
  // here rather than read off the manifest. A band this omits still renders, after these.
  const EQ_BAND_ORDER = ['low', 'mid', 'high'];

  function specsForSlot(slot: string, order: string[]): MixerParamSpec[] {
    const rank = (spec: MixerParamSpec) => {
      const index = order.indexOf(spec.param);
      return index === -1 ? order.length : index;
    };
    return Object.values(deckParams)
      .filter((spec) => spec.slot === slot)
      .sort((left, right) => rank(left) - rank(right));
  }

  const eqSpecs = specsForSlot('eq', EQ_BAND_ORDER);
  const filterSpec = deckParams[FILTER_VALUE];
  const faderSpec = deckParams[FADER_GAIN];

  function defaultParams(): Record<string, number> {
    return Object.fromEntries(
      Object.entries(deckParams).map(([key, spec]) => [key, spec.defaultValue])
    );
  }

  // One entry per deck-scope address the manifest describes, so a param it gains
  // needs no field here, no setter and no command of its own.
  const params = reactive<Record<DeckId, Record<string, number>>>({
    A: defaultParams(),
    B: defaultParams(),
    C: defaultParams(),
    D: defaultParams(),
    E: defaultParams()
  });

  function paramValue(deckId: DeckId, key: string): number {
    return params[deckId][key] ?? deckParams[key]?.defaultValue ?? 0;
  }

  function paramActive(deckId: DeckId, key: string): boolean {
    return paramValue(deckId, key) !== 0;
  }

  function setParam(deckId: DeckId, key: string, value: number): void {
    const spec = deckParams[key];
    if (!spec) return;
    params[deckId][key] = Math.max(spec.min, Math.min(spec.max, value));
    call('set_deck_param', {
      deck: deckId,
      slot: spec.slot,
      param: spec.param,
      value: params[deckId][key]
    });
  }

  function toggleParam(deckId: DeckId, key: string): void {
    setParam(deckId, key, paramActive(deckId, key) ? 0 : 1);
  }

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
    deckCount.value === 2 ? [...TWO_DECK_DISPOSITION] : [...DECKS_DISPOSITION]
  );

  const showWaveformStrip = ref(storageGet<boolean>(STORAGE_KEYS.showWaveformStrip, true));

  function toggleWaveformStrip() {
    showWaveformStrip.value = !showWaveformStrip.value;
    storageSet(STORAGE_KEYS.showWaveformStrip, showWaveformStrip.value);
  }

  const masterGain = ref(defaultMasterGain);

  function setMasterGain(gain: number) {
    masterGain.value = Math.max(0, Math.min(1, gain));
    call('set_master_gain', { gain: masterGain.value });
  }

  const cueMix = ref(0);

  function setCueMix(mix: number) {
    cueMix.value = Math.max(0, Math.min(1, mix));
    call('set_cue_mix', { mix: cueMix.value });
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

  // Per deck, because scrubs overlap: two at once, or one whose end is lost to a window
  // blur, used to restore one deck's volume onto another and leave the second silent.
  const scrubSavedVolume: Partial<Record<DeckId, number>> = {};

  function startScrubMute(deckId: DeckId) {
    if (scrubSavedVolume[deckId] === undefined) {
      scrubSavedVolume[deckId] = paramValue(deckId, FADER_GAIN);
    }
    setParam(deckId, FADER_GAIN, 0);
  }

  function endScrubMute(deckId: DeckId) {
    const saved = scrubSavedVolume[deckId];
    if (saved === undefined) return;
    delete scrubSavedVolume[deckId];
    setParam(deckId, FADER_GAIN, saved);
  }

  function setXfaderPosition(position: number) {
    xfaderPosition.value = Math.max(-1, Math.min(1, position));
    call('set_xfader_position', { position: xfaderPosition.value });
  }

  function setXfaderAssign(deckId: DeckId, assign: XfaderAssign) {
    xfaderAssign[deckId] = assign;
    call('set_xfader_assign', { deck: deckId, assign });
  }

  // The UI has no button for `thru`: the two sides are one exclusive pair, so
  // deselecting the lit one is what takes the deck off the crossfader.
  function toggleXfaderAssign(deckId: DeckId, side: XfaderSide) {
    setXfaderAssign(deckId, xfaderAssign[deckId] === side ? 'thru' : side);
  }

  function setCueActive(deckId: DeckId, active: boolean) {
    cueActive[deckId] = active;
    call('set_cue_active', { deck: deckId, active });
  }

  function paramDefault(key: string): number {
    return deckParams[key]?.defaultValue ?? 0;
  }

  function swarmAffected(deckId: DeckId): DeckId[] {
    const selected = activeDecks.value.filter((candidate) => swarmSelected[candidate]);
    if (!selected.includes(deckId)) selected.push(deckId);
    return selected;
  }

  // A drag carries the gesture's delta to every selected channel, so they keep
  // the offsets the DJ set between them instead of collapsing onto one value.
  function swarmAdjust(deckId: DeckId, key: string, value: number) {
    if (!swarmMode.value) {
      setParam(deckId, key, value);
      return;
    }
    const delta = value - paramValue(deckId, key);
    for (const affected of swarmAffected(deckId)) {
      setParam(affected, key, paramValue(affected, key) + delta);
    }
  }

  function swarmReset(deckId: DeckId, key: string, value: number) {
    const affected = swarmMode.value ? swarmAffected(deckId) : [deckId];
    for (const deck of affected) setParam(deck, key, value);
  }

  function isDeckId(id: string): id is DeckId {
    return Object.prototype.hasOwnProperty.call(params, id);
  }

  function applyEngineParam(change: ParamChange): void {
    // Master scope, so it arrives with no deck and has to be read before the
    // guard below rejects it.
    if (change.slot === 'xfader' && change.param === 'position') {
      xfaderPosition.value = change.value;
      return;
    }
    if (!isDeckId(change.deck)) return;
    // Engine-only routing, so it is the one address `params` cannot hold.
    if (change.slot === 'cue' && change.param === 'active') {
      cueActive[change.deck] = change.value !== 0;
      return;
    }
    const key = paramKey(change.slot, change.param);
    if (deckParams[key]) params[change.deck][key] = change.value;
  }

  listen<ParamChange[]>('engine-params', (event) => {
    event.payload.forEach(applyEngineParam);
  });

  function applyEngineAssign(change: { deck: string; assign: XfaderAssign }): void {
    if (isDeckId(change.deck)) xfaderAssign[change.deck] = change.assign;
  }

  listen<{ deck: string; assign: XfaderAssign }[]>('engine-assign', (event) => {
    event.payload.forEach(applyEngineAssign);
  });

  function reset(): void {
    setXfaderPosition(0);
    for (const deckId of LIVE_DECKS) {
      setXfaderAssign(deckId, 'thru');
      for (const key of Object.keys(deckParams)) setParam(deckId, key, paramDefault(key));
      cueActive[deckId] = false;
      call('set_cue_active', { deck: deckId, active: false });
    }
    engageFiltersIfPreferred();
  }

  function engageFiltersIfPreferred(): void {
    if (!useSettingsStore().filtersEngagedAtStart) return;
    for (const deckId of LIVE_DECKS) setParam(deckId, FILTER_ACTIVE, 1);
  }

  // Keyed on the settings arriving rather than the value, because the preference names the
  // launch and flipping the switch mid-set must not reach into a live mixer either way.
  watch(
    () => useSettingsStore().hydrated,
    (hydrated) => {
      if (hydrated) engageFiltersIfPreferred();
    },
    { immediate: true }
  );

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
            await call('set_cue_device', { deviceId, channelOffset: newCueOffset });
          } catch {
            /* best-effort */
          }
        }
      } else {
        cueDeviceId.value = '';
        try {
          await call('set_cue_device', { deviceId: '', channelOffset: cueChannelOffset.value });
        } catch {
          /* best-effort */
        }
      }
    }
    mainDeviceId.value = deviceId;
    if (channelOffset !== undefined) mainChannelOffset.value = channelOffset;
    try {
      await call('set_main_device', { deviceId, channelOffset: mainChannelOffset.value });
    } catch (e) {
      deviceError.value = `Master out: ${e}`;
    }
  }

  async function getDeckLevels(): Promise<Record<string, [number, number]>> {
    return call('get_deck_levels');
  }

  async function getMasterLevel(): Promise<[number, number]> {
    return call('get_master_level');
  }

  const isRecording = ref(false);
  // Null when no save is running. A WAV is already on disk when the recording
  // stops, so only a FLAC encode ever reports a fraction here.
  const saveProgress = ref<number | null>(null);
  const playedPaths = reactive(new Set<string>());

  function markPlayed(path: string): void {
    playedPaths.add(path);
  }

  async function startRecording(): Promise<void> {
    isRecording.value = true;
    const settings = useSettingsStore();
    const fmt = settings.recordingFormat;
    await call('start_recording', {
      bitDepth: fmt === 'wav-16' ? 16 : 32,
      useFlac: fmt === 'flac',
      // The cue sheet is derived from the event log, so writing one requires
      // capturing the session even when no .bms is kept.
      recordSession: fmt === 'session' || settings.recordBms || settings.recordCue
    });
  }

  // Polled rather than pushed: the audio layer reports permille into an atomic so
  // it never needs an app handle to emit with.
  async function whileReportingSaveProgress<T>(work: () => Promise<T>): Promise<T> {
    const poll = window.setInterval(async () => {
      saveProgress.value = await call('recording_save_progress');
    }, SAVE_POLL_MS);
    try {
      return await work();
    } finally {
      window.clearInterval(poll);
      saveProgress.value = null;
    }
  }

  async function stopRecording(): Promise<string> {
    return whileReportingSaveProgress(async () => {
      const tempPath = await call('stop_recording');
      isRecording.value = false;
      return tempPath;
    });
  }

  async function pickSavePath(baseName: string): Promise<string | null> {
    const settings = useSettingsStore();
    const fmt = settings.recordingFormat;
    const dialogFormat = fmt === 'flac' ? 'flac' : fmt === 'session' ? 'session' : 'wav';
    return call('pick_save_path', { format: dialogFormat, baseName });
  }

  async function saveRecording(src: string, dest: string): Promise<void> {
    const settings = useSettingsStore();
    if (settings.recordingFormat === 'session') {
      await call('save_bms_only', { src, dest });
      return;
    }
    await whileReportingSaveProgress(() =>
      call('save_recording', {
        src,
        dest,
        writeBms: settings.recordBms,
        writeCue: settings.recordCue
      })
    );
  }

  async function discardRecording(path: string): Promise<void> {
    await call('discard_recording', { path });
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
            await call('set_main_device', { deviceId, channelOffset: newMainOffset });
          } catch {
            /* best-effort */
          }
        }
      } else {
        mainDeviceId.value = '';
        try {
          await call('set_main_device', { deviceId: '', channelOffset: mainChannelOffset.value });
        } catch {
          /* best-effort */
        }
      }
    }
    cueDeviceId.value = deviceId;
    if (channelOffset !== undefined) cueChannelOffset.value = channelOffset;
    if (!deviceId) return;
    try {
      await call('set_cue_device', { deviceId, channelOffset: cueChannelOffset.value });
    } catch (e) {
      deviceError.value = `Cue out: ${e}`;
    }
  }

  return {
    startScrubMute,
    endScrubMute,
    activeDecks,
    eqSpecs,
    faderSpec,
    filterSpec,
    cueActive,
    cueChannelOffset,
    cueDeviceId,
    cueMix,
    deckCount,
    deviceError,
    devicesLoaded,
    params,
    paramValue,
    paramActive,
    paramDefault,
    setParam,
    toggleParam,
    mainChannelOffset,
    mainDeviceId,
    masterGain,
    outputDevices,
    showWaveformStrip,
    swarmMode,
    swarmSelected,
    xfaderPosition,
    xfaderAssign,
    isRecording,
    saveProgress,
    playedPaths,
    markPlayed,
    applyEngineParam,
    applyEngineAssign,
    discardRecording,
    getDeckLevels,
    getMasterLevel,
    loadOutputDevices,
    pickSavePath,
    reset,
    saveRecording,
    setCueActive,
    setCueMix,
    setCueOutputDevice,
    setDeckCount,
    setMainOutputDevice,
    setMasterGain,
    setSwarmChannel,
    setSwarmMode,
    swarmAdjust,
    swarmReset,
    setXfaderAssign,
    setXfaderPosition,
    toggleXfaderAssign,
    startRecording,
    stopRecording,
    toggleDeckCount,
    toggleWaveformStrip
  };
});
