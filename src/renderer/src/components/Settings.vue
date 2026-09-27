<template>
  <div class="settings" @click.self="close">
    <div
      ref="modalEl"
      class="settings__modal"
      role="dialog"
      aria-modal="true"
      aria-label="Settings"
    >
      <div class="settings__header">
        <span class="settings__title">{{ $t('settings.title') }}</span>
        <Search
          v-model="filter"
          class="settings__filter"
          :placeholder="$t('settings.filter.placeholder')"
        />
        <button class="settings__close" v-tooltip="$t('settings.close')" @click="close">✕</button>
      </div>

      <div class="settings__body">
        <nav class="settings__nav" role="tablist" :aria-label="$t('settings.title')">
          <button
            v-for="pane in PANES"
            :key="pane.id"
            class="settings__nav-item"
            :class="{ 'settings__nav-item--active': !filtering && activePane === pane.id }"
            role="tab"
            :aria-selected="!filtering && activePane === pane.id"
            @click="selectPane(pane.id)"
          >
            {{ $t(`settings.panes.${pane.id}`) }}
            <span v-if="filtering" class="settings__nav-count">{{ matchCount(pane.id) }}</span>
          </button>
        </nav>

        <div class="settings__panel" role="tabpanel">
          <p v-if="filtering && visibleSections.length === 0" class="settings__empty">
            {{ $t('settings.filter.noMatch') }}
          </p>

          <section v-if="shows('language')" class="settings__section">
            <div class="settings__section-label">{{ $t('settings.language.title') }}</div>
            <div class="settings__row">
              <button
                v-for="lang in LANGUAGES"
                :key="lang.code"
                class="btn-secondary settings__chip"
                :class="{ 'settings__chip--active': locale === lang.code }"
                @click="locale = lang.code"
              >
                {{ lang.label }}
              </button>
            </div>
          </section>

          <section v-if="shows('limiter')" class="settings__section">
            <div class="settings__section-label">{{ $t('settings.limiter.title') }}</div>
            <label class="settings__toggle">
              <input
                type="checkbox"
                :checked="settings.limiterEnabled"
                @change="settings.setLimiterEnabled(($event.target as HTMLInputElement).checked)"
              />
              <span class="settings__toggle-track">
                <span class="settings__toggle-thumb" />
              </span>
              <span class="settings__toggle-label">{{
                settings.limiterEnabled ? $t('settings.limiter.on') : $t('settings.limiter.off')
              }}</span>
            </label>
            <p class="settings__hint">
              {{ $t('settings.limiter.hint') }}
            </p>
          </section>

          <section v-if="shows('nudge')" class="settings__section">
            <div class="settings__section-label">{{ $t('settings.nudge.title') }}</div>
            <div class="settings__row">
              <input
                type="range"
                class="settings__slider"
                min="1"
                max="20"
                step="1"
                :value="settings.nudgeSensitivity"
                @input="settings.setNudgeSensitivity(+($event.target as HTMLInputElement).value)"
              />
              <span class="settings__value">{{ settings.nudgeSensitivity }}%</span>
            </div>
            <p class="settings__hint">{{ $t('settings.nudge.hint') }}</p>
          </section>

          <section v-if="shows('jog')" class="settings__section">
            <div class="settings__section-label">{{ $t('settings.jog.title') }}</div>
            <div class="settings__row">
              <button
                v-for="opt in JOG_ROTATION_SPEED_OPTIONS"
                :key="opt"
                class="btn-secondary settings__chip"
                :class="{ 'settings__chip--active': settings.jogRotationSpeed === opt }"
                @click="settings.setJogRotationSpeed(opt)"
              >
                {{ JOG_ROTATION_SPEED_LABELS[opt] }}
              </button>
            </div>
            <p class="settings__hint">{{ $t('settings.jog.hint') }}</p>
          </section>

          <section v-if="shows('faderCurve')" class="settings__section">
            <div class="settings__section-label">{{ $t('settings.faderCurve.title') }}</div>
            <div class="settings__row">
              <button
                v-for="opt in FADER_CURVE_OPTIONS"
                :key="opt"
                class="btn-secondary settings__chip settings__curve"
                :class="{ 'settings__chip--active': settings.faderCurve === opt }"
                @click="settings.setFaderCurve(opt)"
              >
                <svg class="settings__curve__plot" viewBox="-5 -5 110 110">
                  <polyline :points="curvePlot(opt)" />
                </svg>
                <span>{{ FADER_CURVE_LABELS[opt] }}</span>
              </button>
            </div>
            <p class="settings__hint">{{ $t('settings.faderCurve.hint') }}</p>
          </section>

          <section v-if="shows('sliderClick')" class="settings__section">
            <div class="settings__section-label">{{ $t('settings.sliderClick.title') }}</div>
            <Checkbox
              class="settings__checkbox-row"
              :model-value="settings.sliderClickResets"
              @update:model-value="settings.setSliderClickResets($event)"
            >
              {{ $t('settings.sliderClick.label') }}
            </Checkbox>
            <p class="settings__hint">{{ $t('settings.sliderClick.hint') }}</p>
          </section>

          <section v-if="shows('filterStart')" class="settings__section">
            <div class="settings__section-label">{{ $t('settings.filterStart.title') }}</div>
            <label class="settings__toggle">
              <input
                type="checkbox"
                :checked="settings.filtersEngagedAtStart"
                @change="
                  settings.setFiltersEngagedAtStart(($event.target as HTMLInputElement).checked)
                "
              />
              <span class="settings__toggle-track">
                <span class="settings__toggle-thumb" />
              </span>
              <span class="settings__toggle-label">{{
                settings.filtersEngagedAtStart
                  ? $t('settings.filterStart.on')
                  : $t('settings.filterStart.off')
              }}</span>
            </label>
            <p class="settings__hint">{{ $t('settings.filterStart.hint') }}</p>
          </section>

          <section v-if="shows('pitch')" class="settings__section">
            <div class="settings__section-label">{{ $t('settings.pitch.title') }}</div>
            <div class="settings__row">
              <button
                v-for="opt in PITCH_RANGE_OPTIONS"
                :key="opt"
                class="btn-secondary settings__chip"
                :class="{ 'settings__chip--active': settings.pitchRange === opt }"
                @click="settings.setPitchRange(opt)"
              >
                ±{{ opt }}%
              </button>
            </div>
            <p class="settings__hint">{{ $t('settings.pitch.hint') }}</p>
          </section>

          <section v-if="shows('buffer')" class="settings__section">
            <div class="settings__section-label">{{ $t('settings.buffer.title') }}</div>
            <div class="settings__row">
              <button
                v-for="opt in BUFFER_SIZE_OPTIONS"
                :key="opt"
                class="btn-secondary settings__chip"
                :class="{ 'settings__chip--active': settings.bufferSize === opt }"
                @click="settings.setBufferSize(opt)"
              >
                {{ opt === 0 ? $t('settings.buffer.default') : opt }}
              </button>
            </div>
            <p class="settings__hint">
              {{ $t('settings.buffer.hint') }}
            </p>
          </section>

          <section v-if="shows('midi')" class="settings__section">
            <div class="settings__section-label">{{ $t('settings.midi.title') }}</div>
            <p v-if="midi.devices.length === 0" class="settings__hint">
              {{ $t('settings.midi.noDevices') }}
            </p>
            <div v-for="device in midi.devices" :key="device.port" class="settings__midi-device">
              <div class="settings__midi-device-name">
                <span>{{ device.port }}</span>
                <span class="settings__midi-device-mapping">{{
                  device.mapping ?? $t('settings.midi.unmapped')
                }}</span>
              </div>
              <div v-if="device.assignable" class="settings__row">
                <button
                  v-for="deckId in DECKS_DISPOSITION"
                  :key="deckId"
                  class="btn-secondary settings__chip"
                  :class="{ 'settings__chip--active': device.deck === deckId }"
                  @click="midi.assignDeck(device.port, device.deck === deckId ? null : deckId)"
                >
                  {{ deckId }}
                </button>
              </div>
            </div>
            <div class="settings__row">
              <button class="btn-secondary settings__chip" @click="midi.refresh()">
                {{ $t('settings.midi.rescan') }}
              </button>
            </div>
            <span v-if="midi.error" class="settings__error">{{ midi.error }}</span>
            <p class="settings__hint">
              {{ $t('settings.midi.hint') }}
            </p>
          </section>

          <section v-if="shows('bpmRange')" class="settings__section">
            <div class="settings__section-label">{{ $t('settings.bpmRange.title') }}</div>
            <div class="settings__row">
              <label class="settings__range-label">{{ $t('settings.bpmRange.min') }}</label>
              <input
                type="number"
                class="settings__number"
                min="40"
                max="220"
                step="1"
                :value="settings.bpmMin"
                @change="
                  settings.setBpmRange(+($event.target as HTMLInputElement).value, settings.bpmMax)
                "
              />
              <label class="settings__range-label">{{ $t('settings.bpmRange.max') }}</label>
              <input
                type="number"
                class="settings__number"
                min="41"
                max="250"
                step="1"
                :value="settings.bpmMax"
                @change="
                  settings.setBpmRange(settings.bpmMin, +($event.target as HTMLInputElement).value)
                "
              />
              <span class="settings__value">{{ $t('settings.bpmRange.unit') }}</span>
            </div>
            <p class="settings__hint">
              {{ $t('settings.bpmRange.hint') }}
            </p>
          </section>

          <section v-if="shows('recording')" class="settings__section">
            <div class="settings__section-label">{{ $t('settings.recording.title') }}</div>
            <div class="settings__row">
              <button
                v-for="opt in RECORDING_FORMAT_OPTIONS"
                :key="opt"
                class="btn-secondary settings__chip"
                :class="{ 'settings__chip--active': settings.recordingFormat === opt }"
                @click="settings.setRecordingFormat(opt)"
              >
                {{ RECORDING_FORMAT_LABELS[opt] }}
              </button>
            </div>
            <p class="settings__hint">{{ RECORDING_FORMAT_HINTS[settings.recordingFormat] }}</p>
            <Checkbox
              class="settings__checkbox-row"
              :model-value="settings.recordingFormat === 'session' || settings.recordBms"
              :disabled="settings.recordingFormat === 'session'"
              @update:model-value="settings.setRecordBms($event)"
            >
              {{ $t('settings.recording.bmsCheckbox') }}
            </Checkbox>
            <p class="settings__hint">
              {{ $t('settings.recording.bmsHint') }}
            </p>
            <Checkbox
              class="settings__checkbox-row"
              :model-value="settings.recordingFormat !== 'session' && settings.recordCue"
              :disabled="settings.recordingFormat === 'session'"
              @update:model-value="settings.setRecordCue($event)"
            >
              {{ $t('settings.recording.cueCheckbox') }}
            </Checkbox>
            <p class="settings__hint">
              {{ $t('settings.recording.cueHint') }}
            </p>
          </section>

          <section v-if="shows('deckCount')" class="settings__section">
            <div class="settings__section-label">{{ $t('settings.deckCount.title') }}</div>
            <div class="settings__row">
              <button
                v-for="count in [2, 4] as const"
                :key="count"
                class="btn-secondary settings__chip"
                :class="{ 'settings__chip--active': mixer.deckCount === count }"
                @click="mixer.setDeckCount(count)"
              >
                {{ count }} {{ $t('settings.deckCount.unit') }}
              </button>
            </div>
            <p class="settings__hint">
              {{ $t('settings.deckCount.hint') }}
            </p>
          </section>

          <section v-if="shows('mixer')" class="settings__section">
            <div class="settings__section-label">{{ $t('settings.mixer.title') }}</div>
            <div class="settings__row">
              <button
                v-for="option in MIXER_OPTIONS"
                :key="option.id"
                class="btn-secondary settings__chip"
                :class="{ 'settings__chip--active': mixer.mixerId === option.id }"
                @click="mixer.setMixer(option.id)"
              >
                {{ $t(option.label) }}
              </button>
            </div>
            <p class="settings__hint">
              {{ $t('settings.mixer.hint') }}
            </p>
          </section>

          <section v-if="shows('waveformStyle')" class="settings__section">
            <div class="settings__section-label">{{ $t('settings.waveformStyle.title') }}</div>
            <div class="settings__row">
              <button
                v-for="style in WAVEFORM_STYLE_OPTIONS"
                :key="style"
                class="btn-secondary settings__chip"
                :class="{ 'settings__chip--active': settings.waveformStyle === style }"
                @click="settings.setWaveformStyle(style)"
              >
                {{ $t(`settings.waveformStyle.${style}`) }}
              </button>
            </div>
            <p class="settings__hint">
              {{ $t('settings.waveformStyle.hint') }}
            </p>
          </section>

          <section v-if="shows('deckColors')" class="settings__section">
            <div class="settings__section-label">{{ $t('settings.deckColors.title') }}</div>
            <div class="settings__decks">
              <label
                v-for="deckId in DECKS_DISPOSITION"
                :key="deckId"
                class="settings__deck settings__color-label"
              >
                <span class="settings__deck-label" :style="{ color: accent(deckId) }">
                  {{ $t('settings.keyboard.deck') }} {{ deckId }}
                </span>
                <input
                  type="color"
                  class="settings__color-input settings__color-input--lg"
                  :value="accent(deckId)"
                  @input="decks.setDeckAccent(deckId, ($event.target as HTMLInputElement).value)"
                />
              </label>
            </div>
            <div class="settings__footer" style="margin-top: 4px">
              <span />
              <button class="btn-secondary settings__reset-btn" @click="decks.resetDeckAccents()">
                {{ $t('settings.deckColors.reset') }}
              </button>
            </div>
          </section>

          <section v-if="shows('keyboard')" class="settings__section settings__section--keyboard">
            <div class="settings__section-label">{{ $t('settings.keyboard.title') }}</div>
            <p class="settings__hint">{{ $t('settings.keyboard.hint') }}</p>

            <div ref="decksEl" class="settings__decks" role="grid" @keydown="onGridKeydown">
              <div
                v-for="deckId in DECKS_DISPOSITION"
                :key="decks.decks[deckId].name"
                class="settings__deck"
                role="rowgroup"
              >
                <div class="settings__deck-label" :style="{ color: accent(deckId) }">
                  {{ $t('settings.keyboard.deck') }} {{ deckId }}
                </div>
                <div class="settings__deck-grid" role="row">
                  <template v-for="row in COMMAND_LAYOUT" :key="row[0]">
                    <button
                      v-for="command in row"
                      :key="command"
                      class="settings__btn"
                      :class="{
                        'settings__btn--active': isCapturing(deckId, command),
                        'settings__btn--conflict': isConflict(deckId, command)
                      }"
                      :style="{ '--accent': accent(deckId) }"
                      :aria-label="`${$t('settings.keyboard.deck')} ${deckId} ${COMMAND_LABEL[command]}: ${settings.keybindings[deckId][command]}`"
                      role="gridcell"
                      @click="startCapture(deckId, command)"
                      @dblclick.prevent="resetSlot(deckId, command)"
                    >
                      <span class="settings__btn-key">
                        <template v-if="isCapturing(deckId, command)">···</template>
                        <template v-else>{{ settings.keybindings[deckId][command] }}</template>
                      </span>
                      <span class="settings__btn-action">{{ COMMAND_DISPLAY[command] }}</span>
                    </button>
                  </template>
                </div>
              </div>
            </div>

            <div class="settings__footer">
              <span v-if="conflictError" class="settings__error">{{ conflictError }}</span>
              <span v-else-if="capturingSlot" class="settings__capture-hint">{{
                $t('settings.keyboard.pressKey')
              }}</span>
              <span v-else class="settings__capture-hint" style="opacity: 0" aria-hidden="true"
                >·</span
              >
              <button class="btn-secondary settings__reset-btn" @click="settings.resetToDefaults()">
                {{ $t('settings.keyboard.reset') }}
              </button>
            </div>
          </section>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, watch, onMounted, onUnmounted } from 'vue';
import Checkbox from '@renderer/components/Checkbox.vue';
import Search from '@renderer/components/collection/Search.vue';
import { focusableWithin, trapTabWithin } from '@renderer/utils/focusTrap';
import { markModalClosed, markModalOpen } from '@renderer/utils/modalStack';
import { useI18n } from 'vue-i18n';
import { storageGet, storageSet, STORAGE_KEYS } from '@renderer/utils/storage';
import {
  useSettingsStore,
  PITCH_RANGE_OPTIONS,
  BUFFER_SIZE_OPTIONS,
  RECORDING_FORMAT_OPTIONS,
  JOG_ROTATION_SPEED_OPTIONS,
  FADER_CURVE_OPTIONS,
  type RecordingFormatOption,
  type JogRotationSpeedOption,
  type FaderCurveOption
} from '@renderer/stores/settings';
import { faderCurveGain } from '@renderer/utils/sessionCore';
import { useDecksStore, DECKS_DISPOSITION } from '@renderer/stores/decks';
import { WAVEFORM_STYLE_OPTIONS, type DeckId } from '@renderer/utils/types';
import { useMixerStore } from '@renderer/stores/mixer';
import { SUPPORTED_LOCALES } from '@renderer/i18n';
import { useMidiStore } from '@renderer/stores/midi';
import { commands, resolveKey, DEFAULT_KEYS, type Command } from '@renderer/keybindings';

const { t, te, locale } = useI18n();

const SECTION_PANE: Record<string, PaneId> = {
  buffer: 'audio',
  limiter: 'audio',
  recording: 'audio',
  mixer: 'mixer',
  faderCurve: 'mixer',
  filterStart: 'mixer',
  sliderClick: 'mixer',
  pitch: 'decks',
  nudge: 'decks',
  jog: 'decks',
  bpmRange: 'decks',
  midi: 'control',
  keyboard: 'control',
  language: 'appearance',
  deckCount: 'appearance',
  waveformStyle: 'appearance',
  deckColors: 'appearance'
};

type PaneId = 'audio' | 'mixer' | 'decks' | 'control' | 'appearance';

const PANES: { id: PaneId }[] = [
  { id: 'audio' },
  { id: 'mixer' },
  { id: 'decks' },
  { id: 'control' },
  { id: 'appearance' }
];

const storedPane = storageGet<string>(STORAGE_KEYS.settingsPane, 'audio');
const activePane = ref<PaneId>(
  PANES.some((pane) => pane.id === storedPane) ? (storedPane as PaneId) : 'audio'
);
const filter = ref('');
const filtering = computed(() => filter.value.trim().length > 0);

// Matched on what the reader can see rather than on the key, or a search would only work
// in the language the keys happen to be written in.
function sectionText(section: string): string {
  const parts = [t(`settings.${section}.title`)];
  const hint = `settings.${section}.hint`;
  if (te(hint)) parts.push(t(hint));
  return parts.join(' ').toLowerCase();
}

const visibleSections = computed(() => {
  const query = filter.value.trim().toLowerCase();
  const sections = Object.keys(SECTION_PANE);
  if (!query) return sections.filter((section) => SECTION_PANE[section] === activePane.value);
  return sections.filter((section) => sectionText(section).includes(query));
});

function shows(section: string): boolean {
  return visibleSections.value.includes(section);
}

// A query reaches across every pane, so the counts say where the matches are.
function matchCount(pane: PaneId): number {
  return visibleSections.value.filter((section) => SECTION_PANE[section] === pane).length;
}

function selectPane(pane: PaneId) {
  filter.value = '';
  activePane.value = pane;
  storageSet(STORAGE_KEYS.settingsPane, pane);
}

watch(locale, (val) => storageSet(STORAGE_KEYS.locale, val));

const LANGUAGES = computed(() =>
  SUPPORTED_LOCALES.map((code) => ({ code, label: t(`settings.language.${code}`) }))
);

const JOG_ROTATION_SPEED_LABELS = computed((): Record<JogRotationSpeedOption, string> => ({
  rpm33: t('settings.jog.rpm33'),
  rpm45: t('settings.jog.rpm45')
}));

const FADER_CURVE_LABELS = computed((): Record<FaderCurveOption, string> => ({
  exponential: t('settings.faderCurve.exponential'),
  linear: t('settings.faderCurve.linear'),
  logarithmic: t('settings.faderCurve.logarithmic')
}));

const CURVE_PLOT_SAMPLES = 24;
const CURVE_PLOT_SIZE = 100;

function curvePlot(curve: FaderCurveOption): string {
  return Array.from({ length: CURVE_PLOT_SAMPLES + 1 }, (_unused, index) => {
    const position = index / CURVE_PLOT_SAMPLES;
    const gain = faderCurveGain(curve, position);
    return `${position * CURVE_PLOT_SIZE},${CURVE_PLOT_SIZE - gain * CURVE_PLOT_SIZE}`;
  }).join(' ');
}

const RECORDING_FORMAT_LABELS = computed((): Record<RecordingFormatOption, string> => ({
  'wav-16': t('settings.recording.wav16'),
  'wav-32': t('settings.recording.wav32'),
  flac: t('settings.recording.flac'),
  session: t('settings.recording.sessionOnly')
}));

const RECORDING_FORMAT_HINTS = computed((): Record<RecordingFormatOption, string> => ({
  'wav-16': t('settings.recording.hintWav16'),
  'wav-32': t('settings.recording.hintWav32'),
  flac: t('settings.recording.hintFlac'),
  session: t('settings.recording.hintSession')
}));

const settings = useSettingsStore();
const decks = useDecksStore();
const mixer = useMixerStore();

const MIXER_OPTIONS = [
  { id: 'classic-3band', label: 'settings.mixer.threeBand' },
  { id: 'classic-4band', label: 'settings.mixer.fourBand' }
] as const;
const midi = useMidiStore();

const COMMAND_LAYOUT: [Command, Command][] = [
  [commands.NUDGE_BACK, commands.NUDGE_FORWARD],
  [commands.CUE, commands.PLAY],
  [commands.LOOP_IN, commands.LOOP_OUT_EXIT]
];

const COMMAND_DISPLAY: Record<Command, string> = {
  NUDGE_BACK: '↶',
  NUDGE_FORWARD: '↷',
  CUE: 'CUE',
  PLAY: '▶',
  LOOP_IN: 'IN',
  LOOP_OUT_EXIT: 'OUT'
};

const COMMAND_LABEL = computed((): Record<Command, string> => ({
  NUDGE_BACK: t('settings.keyboard.nudgeLeft'),
  NUDGE_FORWARD: t('settings.keyboard.nudgeRight'),
  CUE: t('settings.keyboard.cue'),
  PLAY: t('settings.keyboard.play'),
  LOOP_IN: t('settings.keyboard.loopIn'),
  LOOP_OUT_EXIT: t('settings.keyboard.loopOut')
}));

function accent(deckId: DeckId): string {
  return decks.decks[deckId]?.accent ?? '#ffffff';
}

type Slot = { deckId: 'A' | 'B' | 'C' | 'D'; command: Command };

const modalEl = ref<HTMLElement | null>(null);
const decksEl = ref<HTMLElement | null>(null);
const capturingSlot = ref<Slot | null>(null);
const conflictSlot = ref<Slot | null>(null);
const conflictError = ref('');
let conflictTimer: ReturnType<typeof setTimeout> | null = null;

function isCapturing(deckId: 'A' | 'B' | 'C' | 'D', command: Command): boolean {
  return capturingSlot.value?.deckId === deckId && capturingSlot.value?.command === command;
}

function isConflict(deckId: 'A' | 'B' | 'C' | 'D', command: Command): boolean {
  return conflictSlot.value?.deckId === deckId && conflictSlot.value?.command === command;
}

function startCapture(deckId: 'A' | 'B' | 'C' | 'D', command: Command) {
  capturingSlot.value = { deckId, command };
  conflictSlot.value = null;
  conflictError.value = '';
  if (conflictTimer) {
    clearTimeout(conflictTimer);
    conflictTimer = null;
  }
}

function resetSlot(deckId: 'A' | 'B' | 'C' | 'D', command: Command) {
  capturingSlot.value = null;
  settings.setKey(deckId, command, DEFAULT_KEYS[deckId][command]);
}

function close() {
  capturingSlot.value = null;
  settings.isOpen = false;
}

const GRID_ROWS = COMMAND_LAYOUT.length;
const GRID_COLS = 2;
const GRID_PER_DECK = GRID_ROWS * GRID_COLS;

function onGridKeydown(e: KeyboardEvent) {
  const isArrow = ['ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown'].includes(e.key);
  const isDelete = e.key === 'Delete' || e.key === 'Backspace';
  if (!isArrow && !isDelete) return;

  const buttons = Array.from(decksEl.value?.querySelectorAll<HTMLElement>('.settings__btn') ?? []);
  const idx = buttons.indexOf(document.activeElement as HTMLElement);
  if (idx === -1) return;

  e.preventDefault();

  const deckIdx = Math.floor(idx / GRID_PER_DECK);
  const posInDeck = idx % GRID_PER_DECK;
  const row = Math.floor(posInDeck / GRID_COLS);
  const col = posInDeck % GRID_COLS;

  if (isDelete) {
    resetSlot(DECKS_DISPOSITION[deckIdx], COMMAND_LAYOUT[row][col]);
    return;
  }

  let nDeck = deckIdx,
    nRow = row,
    nCol = col;
  switch (e.key) {
    case 'ArrowRight':
      if (col < GRID_COLS - 1) nCol = col + 1;
      else if (deckIdx < DECKS_DISPOSITION.length - 1) {
        nDeck = deckIdx + 1;
        nCol = 0;
      }
      break;
    case 'ArrowLeft':
      if (col > 0) nCol = col - 1;
      else if (deckIdx > 0) {
        nDeck = deckIdx - 1;
        nCol = GRID_COLS - 1;
      }
      break;
    case 'ArrowDown':
      if (row < GRID_ROWS - 1) nRow = row + 1;
      break;
    case 'ArrowUp':
      if (row > 0) nRow = row - 1;
      break;
  }

  buttons[nDeck * GRID_PER_DECK + nRow * GRID_COLS + nCol]?.focus();
}

const IGNORED_KEYS = new Set(['Shift', 'Control', 'Alt', 'Meta', 'CapsLock', 'Tab', 'Enter']);

function focusableElements(): HTMLElement[] {
  return focusableWithin(modalEl.value);
}

function onWindowKeydown(e: KeyboardEvent) {
  if (!capturingSlot.value) {
    if (e.key === 'Escape') {
      e.preventDefault();
      close();
      return;
    }
    if (e.key === 'Tab') {
      trapTabWithin(e, modalEl.value);
    }
    return;
  }

  e.preventDefault();
  e.stopImmediatePropagation();

  if (e.key === 'Escape') {
    capturingSlot.value = null;
    return;
  }
  if (IGNORED_KEYS.has(e.key)) return;

  const key = resolveKey(e);
  const { deckId, command } = capturingSlot.value;
  const conflict = settings.setKey(deckId, command, key);

  if (conflict) {
    const conflictDeck = `${t('settings.keyboard.deck')} ${conflict.deckId}`;
    const conflictCmd = COMMAND_LABEL.value[conflict.command];
    conflictError.value = t('settings.keyboard.conflict', {
      key: key.toUpperCase(),
      deck: conflictDeck,
      cmd: conflictCmd
    });
    conflictSlot.value = conflict;
    if (conflictTimer) clearTimeout(conflictTimer);
    conflictTimer = setTimeout(() => {
      conflictError.value = '';
      conflictSlot.value = null;
      conflictTimer = null;
    }, 2500);
    return;
  }

  capturingSlot.value = null;
}

onMounted(async () => {
  markModalOpen();
  window.addEventListener('keydown', onWindowKeydown, { capture: true });
  focusableElements()[0]?.focus();
  await midi.refresh();
});
onUnmounted(() => {
  markModalClosed();
  window.removeEventListener('keydown', onWindowKeydown, { capture: true });
  if (conflictTimer) clearTimeout(conflictTimer);
});
</script>

<style scoped>
/* The overlay's own fade is global (App.vue applies it); the panel's motion stays here. */
.settings.modal-fade-enter-active .settings__modal {
  transition:
    opacity 0.16s ease-out,
    transform 0.16s cubic-bezier(0.2, 0, 0.2, 1);
}

.settings.modal-fade-leave-active .settings__modal {
  transition:
    opacity 0.2s ease-in,
    transform 0.2s cubic-bezier(0.4, 0, 1, 1);
}

.settings.modal-fade-enter-from .settings__modal,
.settings.modal-fade-leave-to .settings__modal {
  opacity: 0;
  transform: translateY(-8px) scale(0.97);
}

@media (prefers-reduced-motion: reduce) {
  .settings.modal-fade-enter-active .settings__modal,
  .settings.modal-fade-leave-active .settings__modal {
    transition-duration: 0.01ms;
  }

  .settings.modal-fade-enter-from .settings__modal,
  .settings.modal-fade-leave-to .settings__modal {
    transform: none;
  }
}

.settings {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.65);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 1000;
}

.settings__modal {
  background: var(--color-bg);
  border: 1px solid var(--color-border);
  border-radius: 6px;
  /* Both axes explicit, because without it the dialog is shrink-to-fit and whichever
     section happens to be largest decides its size, so it resizes as panes change. */
  width: 600px;
  height: 600px;
  max-width: calc(100vw - 32px);
  max-height: calc(100vh - 64px);
  overflow: hidden;
  display: flex;
  flex-direction: column;
  font-family: var(--font);
  box-shadow: 0 24px 64px rgba(0, 0, 0, 0.7);
}

.settings__header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 14px 16px 12px;
  border-bottom: 1px solid var(--color-border);
  flex-shrink: 0;
}

.settings__title {
  font-size: 11px;
  font-weight: 700;
  letter-spacing: 0.03em;
  color: var(--color-text);
  text-transform: uppercase;
}

.settings__close {
  background: transparent;
  border: none;
  color: var(--color-muted);
  font-size: 14px;
  cursor: pointer;
  padding: 0;
  line-height: 1;
  transition: color 0.1s;
}

.settings__close:hover {
  color: var(--color-text);
}

.settings__body {
  flex: 1;
  min-height: 0;
  display: flex;
}

.settings__nav {
  flex-shrink: 0;
  width: 132px;
  border-right: 1px solid var(--color-border);
  padding: 8px 0;
  display: flex;
  flex-direction: column;
  overflow-y: auto;
}

.settings__nav-item {
  background: transparent;
  border: none;
  border-left: 2px solid transparent;
  color: var(--color-muted);
  font-family: var(--font);
  font-size: 11px;
  letter-spacing: 0.03em;
  text-transform: uppercase;
  text-align: left;
  padding: 8px 14px;
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 6px;
  transition: color 0.1s;
}

.settings__nav-item:hover {
  color: var(--color-text);
}

.settings__nav-item--active {
  color: var(--color-text);
  border-left-color: var(--color-text);
}

.settings__nav-count {
  font-size: 10px;
  opacity: 0.6;
}

.settings__panel {
  flex: 1;
  min-width: 0;
  overflow-y: auto;
}

.settings__filter {
  margin-left: auto;
  margin-right: 10px;
}

.settings__empty {
  color: var(--color-muted);
  font-size: 11px;
  padding: 16px;
  margin: 0;
}

.settings__section {
  padding: 16px;
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.settings__section--keyboard {
  gap: 12px;
}

.settings__section + .settings__section {
  border-top: 1px solid var(--color-border);
}

.settings__section-label {
  font-size: 9px;
  font-weight: 700;
  letter-spacing: 0.03em;
  color: var(--color-muted);
  text-transform: uppercase;
}

.settings__hint {
  font-size: 10px;
  color: var(--color-muted);
  margin: 0;
  opacity: 0.65;
}

.settings__checkbox-row {
  font-size: 11px;
  color: var(--color-text);
}

.settings__toggle {
  display: flex;
  align-items: center;
  gap: 8px;
  cursor: pointer;
  user-select: none;
}

.settings__toggle input {
  position: absolute;
  opacity: 0;
  width: 0;
  height: 0;
}

.settings__toggle-track {
  position: relative;
  width: 28px;
  height: 15px;
  background: var(--color-border);
  border-radius: 8px;
  flex-shrink: 0;
  transition: background 0.15s;
}

.settings__toggle input:checked + .settings__toggle-track {
  background: #22c55e;
}

.settings__toggle-thumb {
  position: absolute;
  top: 2px;
  left: 2px;
  width: 11px;
  height: 11px;
  background: #fff;
  border-radius: 50%;
  transition: transform 0.15s;
}

.settings__toggle input:checked + .settings__toggle-track .settings__toggle-thumb {
  transform: translateX(13px);
}

.settings__toggle-label {
  font-size: 10px;
  font-weight: 700;
  letter-spacing: 0.02em;
  color: var(--color-muted);
  min-width: 18px;
  text-transform: uppercase;
}

.settings__row {
  display: flex;
  align-items: center;
  gap: 10px;
}

.settings__slider {
  -webkit-appearance: none;
  appearance: none;
  flex: 1;
  height: 12px;
  background: transparent;
  cursor: pointer;
}

.settings__slider::-webkit-slider-runnable-track {
  height: 3px;
  background: var(--color-border);
  border-radius: 2px;
}

.settings__slider::-webkit-slider-thumb {
  -webkit-appearance: none;
  appearance: none;
  width: 10px;
  height: 16px;
  background: var(--color-text);
  border-radius: 2px;
  margin-top: -6.5px;
}

.settings__value {
  font-size: 10px;
  font-variant-numeric: tabular-nums;
  color: var(--color-muted);
  min-width: 28px;
  text-align: right;
}

.settings__chip {
  /* Declared rather than inherited from the platform: with neither set, the
     button keeps its native appearance, and the active rule's background alone
     drops it to a CSS box a few pixels wider. */
  border: 1px solid var(--color-border);
  background: transparent;
  color: var(--color-muted);
  font-family: var(--font);
  font-size: 10px;
  letter-spacing: var(--label-letter-spacing);
  padding: 3px 7px;
  border-radius: 3px;
  cursor: pointer;
  transition:
    border-color 0.1s,
    color 0.1s,
    background 0.1s;
}

.settings__chip:hover {
  border-color: var(--color-text);
  color: var(--color-text);
}

.settings__chip--active {
  border-color: var(--color-text);
  color: var(--color-text);
  background: color-mix(in srgb, var(--color-text) 8%, transparent);
}

.settings__curve {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 5px;
  padding: 6px 9px;
}

.settings__curve__plot {
  width: 44px;
  height: 44px;
  background: color-mix(in srgb, var(--color-text) 5%, transparent);
  border-radius: 2px;
}

.settings__curve__plot polyline {
  fill: none;
  stroke: currentColor;
  stroke-width: 7;
  stroke-linecap: round;
  stroke-linejoin: round;
}

.settings__range-label {
  font-size: 10px;
  color: var(--color-muted);
  letter-spacing: 0.03em;
}

.settings__number {
  width: 52px;
  background: var(--color-surface);
  border: 1px solid var(--color-border);
  color: var(--color-text);
  font-family: var(--font);
  font-size: 10px;
  font-variant-numeric: tabular-nums;
  padding: 3px 5px;
  border-radius: 3px;
  text-align: center;
}

:root[data-keyboard-nav] .settings__number:focus,
:root[data-keyboard-nav] .settings__color-input:focus {
  outline: 2px solid var(--color-text);
  outline-offset: 2px;
}

.settings__decks {
  display: flex;
  gap: 16px;
}

.settings__deck {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 8px;
  flex: 1;
}

.settings__deck-label {
  font-size: 9px;
  font-weight: 700;
  letter-spacing: 0.03em;
  text-transform: uppercase;
}

.settings__deck-grid {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 4px;
  width: 100%;
}

.settings__btn {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 4px;
  height: 46px;
  border: 1px solid var(--color-border);
  border-radius: 4px;
  background: var(--color-surface);
  color: var(--color-text);
  font-family: var(--font);
  cursor: pointer;
  transition:
    border-color 0.1s,
    background 0.1s,
    color 0.1s;
  padding: 0;
  min-width: 0;
}

.settings__btn:hover {
  border-color: color-mix(in srgb, var(--accent) 60%, transparent);
  background: color-mix(in srgb, var(--accent) 6%, transparent);
}

.settings__btn--active {
  border-color: var(--accent);
  color: var(--accent);
  background: color-mix(in srgb, var(--accent) 10%, transparent);
  animation: btn-pulse 0.75s ease-in-out infinite;
}

.settings__btn--conflict {
  border-color: var(--color-danger);
  color: var(--color-danger);
  background: color-mix(in srgb, var(--color-danger) 10%, transparent);
}

@keyframes btn-pulse {
  0%,
  100% {
    opacity: 1;
  }
  50% {
    opacity: 0.45;
  }
}

.settings__btn-key {
  font-size: 9px;
  font-weight: 700;
  letter-spacing: 0.02em;
  text-transform: uppercase;
  color: var(--accent);
  line-height: 1;
}

.settings__btn--active .settings__btn-key {
  letter-spacing: 0.04em;
}

.settings__btn-action {
  font-size: 12px;
  font-weight: 600;
  color: var(--color-text);
  line-height: 1;
}

.settings__footer {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  min-height: 22px;
}

.settings__midi-device {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding: 4px 0;
}

.settings__midi-device-name {
  display: flex;
  flex-direction: column;
  min-width: 0;
}

.settings__midi-device-mapping {
  font-size: 10px;
  color: var(--color-muted);
}

.settings__error {
  font-size: 10px;
  color: var(--color-danger);
  flex: 1;
}

.settings__capture-hint {
  font-size: 10px;
  color: var(--color-muted);
  opacity: 0.65;
  flex: 1;
}

.settings__reset-btn {
  color: var(--color-muted);
  font-family: var(--font);
  font-size: 10px;
  letter-spacing: 0.03em;
  padding: 3px 8px;
  border-radius: 3px;
  cursor: pointer;
  flex-shrink: 0;
  transition:
    border-color 0.1s,
    color 0.1s;
}

.settings__reset-btn:hover {
  border-color: var(--color-text);
  color: var(--color-text);
}

.settings__color-label {
  cursor: pointer;
}

.settings__color-input {
  -webkit-appearance: none;
  appearance: none;
  width: 32px;
  height: 24px;
  border: 1px solid var(--color-border);
  border-radius: 3px;
  background: none;
  cursor: pointer;
  padding: 2px;
}

.settings__color-input--lg {
  width: 100%;
  height: 36px;
}

.settings__color-input::-webkit-color-swatch-wrapper {
  padding: 0;
}

.settings__color-input::-webkit-color-swatch {
  border: none;
  border-radius: 2px;
}

:root[data-keyboard-nav] .settings__close:focus,
:root[data-keyboard-nav] .settings__reset-btn:focus,
:root[data-keyboard-nav] .settings__chip:focus {
  outline: 2px solid var(--color-text);
  outline-offset: 2px;
}

:root[data-keyboard-nav] .settings__btn:focus {
  outline: 2px solid var(--accent);
  outline-offset: 2px;
}

:root[data-keyboard-nav] .settings__toggle input:focus + .settings__toggle-track {
  outline: 2px solid var(--color-text);
  outline-offset: 2px;
}

:root[data-keyboard-nav] .settings__slider:focus {
  outline: 2px solid var(--color-text);
  outline-offset: 3px;
}
</style>
