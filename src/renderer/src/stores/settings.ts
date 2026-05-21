import { defineStore } from 'pinia';
import { ref, watch } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { load, type Store } from '@tauri-apps/plugin-store';
import { DEFAULT_KEYS, type Keybindings, type Command } from '@renderer/keybindings';

export const PITCH_RANGE_OPTIONS = [6, 8, 10, 16, 50, 100] as const;
export type PitchRangeOption = (typeof PITCH_RANGE_OPTIONS)[number];

export const BUFFER_SIZE_OPTIONS = [0, 128, 256, 512, 1024] as const;
export type BufferSizeOption = (typeof BUFFER_SIZE_OPTIONS)[number];

export const RECORDING_BIT_DEPTH_OPTIONS = [16, 32] as const;
export type RecordingBitDepthOption = (typeof RECORDING_BIT_DEPTH_OPTIONS)[number];

export const RECORDING_FORMAT_OPTIONS = ['wav', 'flac'] as const;
export type RecordingFormatOption = (typeof RECORDING_FORMAT_OPTIONS)[number];

type Stored = {
  keybindings?: Keybindings;
  limiterEnabled?: boolean;
  nudgeSensitivity?: number;
  pitchRange?: PitchRangeOption;
  bufferSize?: BufferSizeOption;
  bpmMin?: number;
  bpmMax?: number;
  recordingBitDepth?: RecordingBitDepthOption;
  recordingFormat?: RecordingFormatOption;
  recordSession?: boolean;
};

export type ConflictInfo = { deckId: 'A' | 'B' | 'C' | 'D'; command: Command };

export const useSettingsStore = defineStore('settings', () => {
  const keybindings = ref<Keybindings>(structuredClone(DEFAULT_KEYS));
  const limiterEnabled = ref<boolean>(true);
  const nudgeSensitivity = ref<number>(4);
  const pitchRange = ref<PitchRangeOption>(10);
  const bufferSize = ref<BufferSizeOption>(0);
  const bpmMin = ref<number>(90);
  const bpmMax = ref<number>(180);
  const recordingBitDepth = ref<RecordingBitDepthOption>(32);
  const recordingFormat = ref<RecordingFormatOption>('wav');
  const recordSession = ref<boolean>(false);
  const isOpen = ref(false);

  let store: Store | null = null;

  function applyStored(stored: Stored): void {
    if (stored.keybindings) keybindings.value = stored.keybindings;
    limiterEnabled.value = stored.limiterEnabled ?? limiterEnabled.value;
    nudgeSensitivity.value = stored.nudgeSensitivity ?? nudgeSensitivity.value;
    pitchRange.value = stored.pitchRange ?? pitchRange.value;
    bufferSize.value = stored.bufferSize ?? bufferSize.value;
    bpmMin.value = stored.bpmMin ?? bpmMin.value;
    bpmMax.value = stored.bpmMax ?? bpmMax.value;
    recordingBitDepth.value = stored.recordingBitDepth ?? recordingBitDepth.value;
    recordingFormat.value = stored.recordingFormat ?? recordingFormat.value;
    recordSession.value = stored.recordSession ?? recordSession.value;
  }

  async function init(): Promise<void> {
    try {
      store = await load('settings.json', { autoSave: false, defaults: {} });
      const saved = await store.get<Stored>('v1');
      if (saved) applyStored(saved);
    } catch {
      // use defaults
    }
  }

  async function save(): Promise<void> {
    if (!store) return;
    await store.set('v1', {
      keybindings: keybindings.value,
      limiterEnabled: limiterEnabled.value,
      nudgeSensitivity: nudgeSensitivity.value,
      pitchRange: pitchRange.value,
      bufferSize: bufferSize.value,
      bpmMin: bpmMin.value,
      bpmMax: bpmMax.value,
      recordingBitDepth: recordingBitDepth.value,
      recordingFormat: recordingFormat.value,
      recordSession: recordSession.value
    } satisfies Stored);
    await store.save();
  }

  async function trySave(): Promise<void> {
    try {
      await save();
    } catch {
      // save failures are non-fatal
    }
  }

  function setKey(
    deckId: 'A' | 'B' | 'C' | 'D',
    command: Command,
    key: string
  ): ConflictInfo | null {
    for (const [d, bindings] of Object.entries(keybindings.value) as [
      'A' | 'B' | 'C' | 'D',
      Record<Command, string>
    ][]) {
      for (const [cmd, bound] of Object.entries(bindings) as [Command, string][]) {
        if (bound === key && !(d === deckId && cmd === command)) {
          return { deckId: d, command: cmd };
        }
      }
    }
    keybindings.value = {
      ...keybindings.value,
      [deckId]: { ...keybindings.value[deckId], [command]: key }
    };
    trySave();
    return null;
  }

  function resetToDefaults(): void {
    keybindings.value = structuredClone(DEFAULT_KEYS);
    trySave();
  }

  watch(limiterEnabled, (v) => invoke('set_limiter_enabled', { enabled: v }), { immediate: true });
  watch(bufferSize, (v) => invoke('set_buffer_size', { frames: v }), { immediate: true });
  watch([bpmMin, bpmMax], ([min, max]) => invoke('set_bpm_range', { min, max }), {
    immediate: true
  });

  function setLimiterEnabled(enabled: boolean): void {
    limiterEnabled.value = enabled;
    trySave();
  }

  function setNudgeSensitivity(value: number): void {
    nudgeSensitivity.value = Math.max(1, Math.min(20, value));
    trySave();
  }

  function setPitchRange(value: PitchRangeOption): void {
    pitchRange.value = value;
    trySave();
  }

  function setBufferSize(value: BufferSizeOption): void {
    bufferSize.value = value;
    trySave();
  }

  function setBpmRange(min: number, max: number): void {
    bpmMin.value = Math.max(40, Math.min(min, max - 1));
    bpmMax.value = Math.min(250, Math.max(max, min + 1));
    trySave();
  }

  function setRecordingBitDepth(value: RecordingBitDepthOption): void {
    recordingBitDepth.value = value;
    trySave();
  }

  function setRecordingFormat(value: RecordingFormatOption): void {
    recordingFormat.value = value;
    trySave();
  }

  function setRecordSession(value: boolean): void {
    recordSession.value = value;
    trySave();
  }

  return {
    bpmMax,
    bpmMin,
    bufferSize,
    isOpen,
    keybindings,
    limiterEnabled,
    nudgeSensitivity,
    pitchRange,
    recordingBitDepth,
    recordingFormat,
    recordSession,
    init,
    resetToDefaults,
    setBpmRange,
    setBufferSize,
    setKey,
    setLimiterEnabled,
    setNudgeSensitivity,
    setPitchRange,
    setRecordingBitDepth,
    setRecordingFormat,
    setRecordSession
  };
});
