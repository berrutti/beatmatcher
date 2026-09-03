import { defineStore } from 'pinia';
import { ref, watch } from 'vue';
import { call } from '@renderer/tauriCommands';
import { load, type Store } from '@tauri-apps/plugin-store';
import { DEFAULT_KEYS, type Keybindings, type Command } from '@renderer/keybindings';
import type { WaveformStyleOption } from '@renderer/utils/types';

export const PITCH_RANGE_OPTIONS = [6, 8, 10, 16, 50, 100] as const;
export type PitchRangeOption = (typeof PITCH_RANGE_OPTIONS)[number];

export const BUFFER_SIZE_OPTIONS = [0, 128, 256, 512, 1024] as const;
export type BufferSizeOption = (typeof BUFFER_SIZE_OPTIONS)[number];

export const RECORDING_FORMAT_OPTIONS = ['wav-16', 'wav-32', 'flac', 'session'] as const;
export type RecordingFormatOption = (typeof RECORDING_FORMAT_OPTIONS)[number];

export const JOG_ROTATION_SPEED_OPTIONS = ['rpm33', 'rpm45'] as const;
export type JogRotationSpeedOption = (typeof JOG_ROTATION_SPEED_OPTIONS)[number];

export const FADER_CURVE_OPTIONS = ['exponential', 'linear', 'logarithmic'] as const;
export type FaderCurveOption = (typeof FADER_CURVE_OPTIONS)[number];

// Sessions written before the .bms carried a mixer header were all played on
// this one, so it is the fallback when a file names none.
export const DEFAULT_MIXER_ID = 'classic-3band';

// What the engine builds its strips on today. It diverged from the fallback with the
// crossfader, which could not go into a frozen manifest without refusing older sessions.
export const LIVE_MIXER_ID = 'classic-3band-v2';

type Stored = {
  keybindings?: Keybindings;
  limiterEnabled?: boolean;
  sliderClickResets?: boolean;
  nudgeSensitivity?: number;
  jogRotationSpeed?: JogRotationSpeedOption;
  faderCurve?: FaderCurveOption;
  filtersEngagedAtStart?: boolean;
  pitchRange?: PitchRangeOption;
  bufferSize?: BufferSizeOption;
  bpmMin?: number;
  bpmMax?: number;
  recordingFormat?: RecordingFormatOption;
  recordBms?: boolean;
  recordCue?: boolean;
  deckAccents?: Record<string, string>;
  waveformStyle?: WaveformStyleOption;
};

export type ConflictInfo = { deckId: 'A' | 'B' | 'C' | 'D'; command: Command };

export const useSettingsStore = defineStore('settings', () => {
  const keybindings = ref<Keybindings>(structuredClone(DEFAULT_KEYS));
  const limiterEnabled = ref<boolean>(true);
  // On by default: a fader you can return to full with one click is the reason
  // it is there. Distracting enough for some that it stays opt-out.
  const sliderClickResets = ref<boolean>(true);
  const nudgeSensitivity = ref<number>(4);
  const jogRotationSpeed = ref<JogRotationSpeedOption>('rpm33');
  const faderCurve = ref<FaderCurveOption>('linear');
  const filtersEngagedAtStart = ref<boolean>(false);
  const pitchRange = ref<PitchRangeOption>(10);
  const bufferSize = ref<BufferSizeOption>(0);
  const bpmMin = ref<number>(90);
  const bpmMax = ref<number>(180);
  const recordingFormat = ref<RecordingFormatOption>('wav-32');
  const recordBms = ref<boolean>(false);
  const recordCue = ref<boolean>(false);
  const deckAccents = ref<Record<string, string>>({});
  const waveformStyle = ref<WaveformStyleOption>('threeBand');
  const isOpen = ref(false);
  // Distinguishes the stored values arriving from the user changing one, which a
  // setting that only applies at launch has to be able to tell apart.
  const hydrated = ref(false);

  let store: Store | null = null;

  function applyStored(stored: Stored): void {
    if (stored.keybindings) keybindings.value = stored.keybindings;
    limiterEnabled.value = stored.limiterEnabled ?? limiterEnabled.value;
    sliderClickResets.value = stored.sliderClickResets ?? sliderClickResets.value;
    nudgeSensitivity.value = stored.nudgeSensitivity ?? nudgeSensitivity.value;
    jogRotationSpeed.value = stored.jogRotationSpeed ?? jogRotationSpeed.value;
    faderCurve.value = stored.faderCurve ?? faderCurve.value;
    filtersEngagedAtStart.value = stored.filtersEngagedAtStart ?? filtersEngagedAtStart.value;
    pitchRange.value = stored.pitchRange ?? pitchRange.value;
    bufferSize.value = stored.bufferSize ?? bufferSize.value;
    bpmMin.value = stored.bpmMin ?? bpmMin.value;
    bpmMax.value = stored.bpmMax ?? bpmMax.value;
    recordingFormat.value = stored.recordingFormat ?? recordingFormat.value;
    recordBms.value = stored.recordBms ?? recordBms.value;
    recordCue.value = stored.recordCue ?? recordCue.value;
    deckAccents.value = stored.deckAccents ?? deckAccents.value;
    waveformStyle.value = stored.waveformStyle ?? waveformStyle.value;
  }

  async function init(): Promise<void> {
    try {
      store = await load('settings.json', { autoSave: false, defaults: {} });
      const saved = await store.get<Stored>('v1');
      if (saved) applyStored(saved);
    } catch {
      // use defaults
    }
    hydrated.value = true;
  }

  async function save(): Promise<void> {
    if (!store) return;
    await store.set('v1', {
      keybindings: keybindings.value,
      limiterEnabled: limiterEnabled.value,
      sliderClickResets: sliderClickResets.value,
      nudgeSensitivity: nudgeSensitivity.value,
      jogRotationSpeed: jogRotationSpeed.value,
      faderCurve: faderCurve.value,
      filtersEngagedAtStart: filtersEngagedAtStart.value,
      pitchRange: pitchRange.value,
      bufferSize: bufferSize.value,
      bpmMin: bpmMin.value,
      bpmMax: bpmMax.value,
      recordingFormat: recordingFormat.value,
      recordBms: recordBms.value,
      recordCue: recordCue.value,
      deckAccents: Object.keys(deckAccents.value).length > 0 ? deckAccents.value : undefined,
      waveformStyle: waveformStyle.value
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

  watch(limiterEnabled, (v) => call('set_limiter_enabled', { enabled: v }), { immediate: true });
  watch(bufferSize, (v) => call('set_buffer_size', { frames: v }), { immediate: true });
  watch([bpmMin, bpmMax], ([min, max]) => call('set_bpm_range', { min, max }), {
    immediate: true
  });
  watch(pitchRange, (v) => call('set_pitch_range', { percent: v }), { immediate: true });
  watch(jogRotationSpeed, (v) => call('set_jog_rotation_speed', { speed: v }), {
    immediate: true
  });
  watch(faderCurve, (v) => call('set_fader_curve', { curve: v }), { immediate: true });

  function setSliderClickResets(enabled: boolean): void {
    sliderClickResets.value = enabled;
    trySave();
  }

  function setLimiterEnabled(enabled: boolean): void {
    limiterEnabled.value = enabled;
    trySave();
  }

  function setNudgeSensitivity(value: number): void {
    nudgeSensitivity.value = Math.max(1, Math.min(20, value));
    trySave();
  }

  function setJogRotationSpeed(value: JogRotationSpeedOption): void {
    jogRotationSpeed.value = value;
    trySave();
  }

  function setFaderCurve(value: FaderCurveOption): void {
    faderCurve.value = value;
    trySave();
  }

  function setFiltersEngagedAtStart(value: boolean): void {
    filtersEngagedAtStart.value = value;
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

  function setRecordingFormat(value: RecordingFormatOption): void {
    recordingFormat.value = value;
    trySave();
  }

  function setRecordBms(value: boolean): void {
    recordBms.value = value;
    trySave();
  }

  function setRecordCue(value: boolean): void {
    recordCue.value = value;
    trySave();
  }

  function setDeckAccents(accents: Record<string, string>): void {
    deckAccents.value = accents;
    trySave();
  }

  function setWaveformStyle(value: WaveformStyleOption): void {
    waveformStyle.value = value;
    trySave();
  }

  return {
    bpmMax,
    bpmMin,
    bufferSize,
    deckAccents,
    waveformStyle,
    setWaveformStyle,
    faderCurve,
    filtersEngagedAtStart,
    isOpen,
    hydrated,
    jogRotationSpeed,
    keybindings,
    limiterEnabled,
    sliderClickResets,
    nudgeSensitivity,
    pitchRange,
    recordingFormat,
    recordBms,
    recordCue,
    init,
    resetToDefaults,
    setBpmRange,
    setBufferSize,
    setFaderCurve,
    setFiltersEngagedAtStart,
    setJogRotationSpeed,
    setKey,
    setLimiterEnabled,
    setSliderClickResets,
    setNudgeSensitivity,
    setPitchRange,
    setRecordingFormat,
    setRecordBms,
    setRecordCue,
    setDeckAccents
  };
});
