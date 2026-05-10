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
  const isOpen = ref(false);

  let _store: Store | null = null;

  function applyStored(stored: Stored): void {
    if (stored.keybindings) keybindings.value = stored.keybindings;
    if (stored.limiterEnabled !== undefined) limiterEnabled.value = stored.limiterEnabled;
    if (stored.nudgeSensitivity !== undefined) nudgeSensitivity.value = stored.nudgeSensitivity;
    if (stored.pitchRange !== undefined) pitchRange.value = stored.pitchRange;
    if (stored.bufferSize !== undefined) bufferSize.value = stored.bufferSize;
    if (stored.bpmMin !== undefined) bpmMin.value = stored.bpmMin;
    if (stored.bpmMax !== undefined) bpmMax.value = stored.bpmMax;
    if (stored.recordingBitDepth !== undefined) recordingBitDepth.value = stored.recordingBitDepth;
    if (stored.recordingFormat !== undefined) recordingFormat.value = stored.recordingFormat;
  }

  async function init(): Promise<void> {
    try {
      _store = await load('settings.json', { autoSave: false, defaults: {} });
      const saved = await _store.get<Stored>('v1');
      if (saved) applyStored(saved);
    } catch {
      // use defaults
    }
  }

  async function save(): Promise<void> {
    if (!_store) return;
    await _store.set('v1', {
      keybindings: keybindings.value,
      limiterEnabled: limiterEnabled.value,
      nudgeSensitivity: nudgeSensitivity.value,
      pitchRange: pitchRange.value,
      bufferSize: bufferSize.value,
      bpmMin: bpmMin.value,
      bpmMax: bpmMax.value,
      recordingBitDepth: recordingBitDepth.value,
      recordingFormat: recordingFormat.value
    } satisfies Stored);
    await _store.save();
  }

  function saveAsync(): void {
    save().catch(() => {});
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
    saveAsync();
    return null;
  }

  function resetToDefaults(): void {
    keybindings.value = structuredClone(DEFAULT_KEYS);
    saveAsync();
  }

  watch(limiterEnabled, (v) => invoke('set_limiter_enabled', { enabled: v }), { immediate: true });
  watch(bufferSize, (v) => invoke('set_buffer_size', { frames: v }), { immediate: true });
  watch([bpmMin, bpmMax], ([min, max]) => invoke('set_bpm_range', { min, max }), {
    immediate: true
  });

  function setLimiterEnabled(enabled: boolean): void {
    limiterEnabled.value = enabled;
    saveAsync();
  }

  function setNudgeSensitivity(value: number): void {
    nudgeSensitivity.value = Math.max(1, Math.min(20, value));
    saveAsync();
  }

  function setPitchRange(value: PitchRangeOption): void {
    pitchRange.value = value;
    saveAsync();
  }

  function setBufferSize(value: BufferSizeOption): void {
    bufferSize.value = value;
    saveAsync();
  }

  function setBpmRange(min: number, max: number): void {
    bpmMin.value = Math.max(40, Math.min(min, max - 1));
    bpmMax.value = Math.min(250, Math.max(max, min + 1));
    saveAsync();
  }

  function setRecordingBitDepth(value: RecordingBitDepthOption): void {
    recordingBitDepth.value = value;
    saveAsync();
  }

  function setRecordingFormat(value: RecordingFormatOption): void {
    recordingFormat.value = value;
    saveAsync();
  }

  return {
    keybindings,
    limiterEnabled,
    nudgeSensitivity,
    pitchRange,
    bufferSize,
    bpmMin,
    bpmMax,
    recordingBitDepth,
    recordingFormat,
    isOpen,
    init,
    setKey,
    resetToDefaults,
    setLimiterEnabled,
    setNudgeSensitivity,
    setPitchRange,
    setBufferSize,
    setBpmRange,
    setRecordingBitDepth,
    setRecordingFormat
  };
});
