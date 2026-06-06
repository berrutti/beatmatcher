<template>
  <div class="settings-overlay" @click.self="close">
    <div ref="modalEl" class="settings-modal" role="dialog" aria-modal="true" aria-label="Settings">
      <div class="settings-header">
        <span class="settings-title">SETTINGS</span>
        <button class="settings-close" title="Close" @click="close">✕</button>
      </div>

      <section class="settings-section">
        <div class="settings-section-label">MASTER LIMITER</div>
        <label class="settings-toggle">
          <input
            type="checkbox"
            :checked="settings.limiterEnabled"
            @change="settings.setLimiterEnabled(($event.target as HTMLInputElement).checked)"
          />
          <span class="settings-toggle-track">
            <span class="settings-toggle-thumb" />
          </span>
          <span class="settings-toggle-label">{{ settings.limiterEnabled ? 'ON' : 'OFF' }}</span>
        </label>
        <p class="settings-hint">
          Prevents digital clipping on the master output. Disable only if using an external limiter.
        </p>
      </section>

      <section class="settings-section">
        <div class="settings-section-label">NUDGE SENSITIVITY</div>
        <div class="settings-row">
          <input
            type="range"
            class="settings-slider"
            min="1"
            max="20"
            step="1"
            :value="settings.nudgeSensitivity"
            @input="settings.setNudgeSensitivity(+($event.target as HTMLInputElement).value)"
          />
          <span class="settings-value">{{ settings.nudgeSensitivity }}%</span>
        </div>
        <p class="settings-hint">Speed offset applied while holding a nudge key or button.</p>
      </section>

      <section class="settings-section">
        <div class="settings-section-label">PITCH RANGE</div>
        <div class="settings-row">
          <button
            v-for="opt in PITCH_RANGE_OPTIONS"
            :key="opt"
            class="settings-chip"
            :class="{ 'settings-chip--active': settings.pitchRange === opt }"
            @click="settings.setPitchRange(opt)"
          >
            ±{{ opt }}%
          </button>
        </div>
        <p class="settings-hint">Maximum pitch slider deviation from original BPM.</p>
      </section>

      <section class="settings-section">
        <div class="settings-section-label">AUDIO BUFFER SIZE</div>
        <div class="settings-row">
          <button
            v-for="opt in BUFFER_SIZE_OPTIONS"
            :key="opt"
            class="settings-chip"
            :class="{ 'settings-chip--active': settings.bufferSize === opt }"
            @click="settings.setBufferSize(opt)"
          >
            {{ opt === 0 ? 'Default' : opt }}
          </button>
        </div>
        <p class="settings-hint">
          Frames per audio callback. Smaller values reduce latency but increase CPU load. Changing
          this causes a brief audio gap.
        </p>
      </section>

      <section class="settings-section">
        <div class="settings-section-label">BPM DETECTION RANGE</div>
        <div class="settings-row">
          <label class="settings-range-label">Min</label>
          <input
            type="number"
            class="settings-number"
            min="40"
            max="220"
            step="1"
            :value="settings.bpmMin"
            @change="
              settings.setBpmRange(+($event.target as HTMLInputElement).value, settings.bpmMax)
            "
          />
          <label class="settings-range-label">Max</label>
          <input
            type="number"
            class="settings-number"
            min="41"
            max="250"
            step="1"
            :value="settings.bpmMax"
            @change="
              settings.setBpmRange(settings.bpmMin, +($event.target as HTMLInputElement).value)
            "
          />
          <span class="settings-value">BPM</span>
        </div>
        <p class="settings-hint">
          Tracks detected outside this range will be doubled or halved until they fit. Useful for
          genres with tempos outside the 90-180 default.
        </p>
      </section>

      <section class="settings-section">
        <div class="settings-section-label">RECORDING FORMAT</div>
        <div class="settings-row">
          <button
            v-for="opt in RECORDING_FORMAT_OPTIONS"
            :key="opt"
            class="settings-chip"
            :class="{ 'settings-chip--active': settings.recordingFormat === opt }"
            @click="settings.setRecordingFormat(opt)"
          >
            {{ RECORDING_FORMAT_LABELS[opt] }}
          </button>
        </div>
        <p class="settings-hint">{{ RECORDING_FORMAT_HINTS[settings.recordingFormat] }}</p>
        <label
          class="settings-checkbox-row"
          :class="{ 'settings-checkbox-row--disabled': settings.recordingFormat === 'session' }"
        >
          <input
            type="checkbox"
            :checked="settings.recordingFormat === 'session' || settings.recordBms"
            :disabled="settings.recordingFormat === 'session'"
            @change="settings.setRecordBms(($event.target as HTMLInputElement).checked)"
          />
          <span>Always record a .bms alongside audio</span>
        </label>
        <p class="settings-hint">
          Saves a .bms file with a full event log of every action taken during the set. Required for
          rendering or replaying a session.
        </p>
      </section>

      <section class="settings-section">
        <div class="settings-section-label">DEFAULT DECK COUNT</div>
        <div class="settings-row">
          <button
            v-for="count in [2, 4] as const"
            :key="count"
            class="settings-chip"
            :class="{ 'settings-chip--active': mixer.deckCount === count }"
            @click="mixer.setDeckCount(count)"
          >
            {{ count }} decks
          </button>
        </div>
        <p class="settings-hint">
          Number of active decks. Takes effect immediately and is remembered across sessions.
        </p>
      </section>

      <section class="settings-section settings-section--keyboard">
        <div class="settings-section-label">KEYBOARD MAPPING</div>
        <p class="settings-hint">
          Click or press Enter to remap. Arrow keys navigate. Delete resets to default. Double-click
          also resets.
        </p>

        <div ref="decksEl" class="settings-decks" role="grid" @keydown="onGridKeydown">
          <div
            v-for="deckId in DECKS_DISPOSITION"
            :key="decks.decks[deckId].name"
            class="settings-deck"
            role="rowgroup"
          >
            <div class="settings-deck-label" :style="{ color: accent(deckId) }">
              DECK {{ deckId }}
            </div>
            <div class="settings-deck-grid" role="row">
              <template v-for="row in COMMAND_LAYOUT" :key="row[0]">
                <button
                  v-for="command in row"
                  :key="command"
                  class="settings-btn"
                  :class="{
                    'settings-btn--active': isCapturing(deckId, command),
                    'settings-btn--conflict': isConflict(deckId, command)
                  }"
                  :style="{ '--accent': accent(deckId) }"
                  :aria-label="`Deck ${deckId} ${COMMAND_LABEL[command]}: ${settings.keybindings[deckId][command]}`"
                  role="gridcell"
                  @click="startCapture(deckId, command)"
                  @dblclick.prevent="resetSlot(deckId, command)"
                >
                  <span class="settings-btn-key">
                    <template v-if="isCapturing(deckId, command)">···</template>
                    <template v-else>{{ settings.keybindings[deckId][command] }}</template>
                  </span>
                  <span class="settings-btn-action">{{ COMMAND_DISPLAY[command] }}</span>
                </button>
              </template>
            </div>
          </div>
        </div>

        <div class="settings-footer">
          <span v-if="conflictError" class="settings-error">{{ conflictError }}</span>
          <span v-else-if="capturingSlot" class="settings-capture-hint"
            >Press any key. Esc to cancel</span
          >
          <span v-else class="settings-capture-hint" style="opacity: 0" aria-hidden="true">·</span>
          <button class="settings-reset-btn" @click="settings.resetToDefaults()">
            Reset to defaults
          </button>
        </div>
      </section>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted, onUnmounted } from 'vue';
import {
  useSettingsStore,
  PITCH_RANGE_OPTIONS,
  BUFFER_SIZE_OPTIONS,
  RECORDING_FORMAT_OPTIONS,
  type RecordingFormatOption
} from '@renderer/stores/settings';
import { useDecksStore, DECKS_DISPOSITION, type DeckId } from '@renderer/stores/decks';
import { useMixerStore } from '@renderer/stores/mixer';
import { commands, resolveKey, DEFAULT_KEYS, type Command } from '@renderer/keybindings';

const RECORDING_FORMAT_LABELS: Record<RecordingFormatOption, string> = {
  'wav-16': 'WAV (16-bit)',
  'wav-32': 'WAV (32-bit)',
  flac: 'FLAC',
  session: 'SESSION ONLY'
};

const RECORDING_FORMAT_HINTS: Record<RecordingFormatOption, string> = {
  'wav-16': 'Uncompressed PCM. Standard CD quality, smaller files than 32-bit.',
  'wav-32': 'Uncompressed 32-bit float. Full dynamic range, largest files.',
  flac: 'Lossless compression, encoded after you stop. Same quality as WAV, smaller files.',
  session:
    'Records only the event log. No audio file is saved. You can render to WAV or FLAC later from the session view.'
};

const settings = useSettingsStore();
const decks = useDecksStore();
const mixer = useMixerStore();

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

const COMMAND_LABEL: Record<Command, string> = {
  NUDGE_BACK: 'Nudge ←',
  NUDGE_FORWARD: 'Nudge →',
  CUE: 'CUE',
  PLAY: 'PLAY',
  LOOP_IN: 'Loop IN',
  LOOP_OUT_EXIT: 'Loop OUT'
};

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

  const buttons = Array.from(decksEl.value?.querySelectorAll<HTMLElement>('.settings-btn') ?? []);
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
  return Array.from(
    modalEl.value?.querySelectorAll<HTMLElement>(
      'button:not([disabled]), input:not([disabled]), select:not([disabled])'
    ) ?? []
  );
}

function onWindowKeydown(e: KeyboardEvent) {
  if (!capturingSlot.value) {
    if (e.key === 'Escape') {
      e.preventDefault();
      close();
      return;
    }
    if (e.key === 'Tab') {
      const els = focusableElements();
      if (els.length === 0) return;
      const first = els[0];
      const last = els[els.length - 1];
      if (e.shiftKey && document.activeElement === first) {
        e.preventDefault();
        last.focus();
      } else if (!e.shiftKey && document.activeElement === last) {
        e.preventDefault();
        first.focus();
      }
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
    const conflictDeck = `Deck ${conflict.deckId}`;
    const conflictCmd = COMMAND_LABEL[conflict.command];
    conflictError.value = `'${key.toUpperCase()}' is already used by ${conflictDeck} / ${conflictCmd}`;
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

onMounted(() => {
  window.addEventListener('keydown', onWindowKeydown, { capture: true });
  focusableElements()[0]?.focus();
});
onUnmounted(() => {
  window.removeEventListener('keydown', onWindowKeydown, { capture: true });
  if (conflictTimer) clearTimeout(conflictTimer);
});
</script>

<style scoped>
.settings-overlay {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.65);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 1000;
}

.settings-modal {
  background: var(--color-bg);
  border: 1px solid var(--color-border);
  border-radius: 6px;
  max-width: calc(100vw - 32px);
  max-height: calc(100vh - 64px);
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  font-family: var(--font);
  box-shadow: 0 24px 64px rgba(0, 0, 0, 0.7);
}

.settings-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 14px 16px 12px;
  border-bottom: 1px solid var(--color-border);
  flex-shrink: 0;
}

.settings-title {
  font-size: 11px;
  font-weight: 700;
  letter-spacing: 0.2em;
  color: var(--color-text);
}

.settings-close {
  background: transparent;
  border: none;
  color: var(--color-muted);
  font-size: 14px;
  cursor: pointer;
  padding: 0;
  line-height: 1;
  transition: color 0.1s;
}

.settings-close:hover {
  color: var(--color-text);
}

.settings-section {
  padding: 16px;
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.settings-section--keyboard {
  gap: 12px;
}

.settings-section + .settings-section {
  border-top: 1px solid var(--color-border);
}

.settings-section-label {
  font-size: 9px;
  font-weight: 700;
  letter-spacing: 0.2em;
  color: var(--color-muted);
}

.settings-hint {
  font-size: 10px;
  color: var(--color-muted);
  margin: 0;
  opacity: 0.65;
}

.settings-checkbox-row {
  display: flex;
  align-items: center;
  gap: 8px;
  cursor: pointer;
  font-size: 11px;
  color: var(--color-text);
  user-select: none;
}

.settings-checkbox-row--disabled {
  opacity: 0.4;
  cursor: default;
  pointer-events: none;
}

.settings-toggle {
  display: flex;
  align-items: center;
  gap: 8px;
  cursor: pointer;
  user-select: none;
}

.settings-toggle input {
  position: absolute;
  opacity: 0;
  width: 0;
  height: 0;
}

.settings-toggle-track {
  position: relative;
  width: 28px;
  height: 15px;
  background: var(--color-border);
  border-radius: 8px;
  flex-shrink: 0;
  transition: background 0.15s;
}

.settings-toggle input:checked + .settings-toggle-track {
  background: #22c55e;
}

.settings-toggle-thumb {
  position: absolute;
  top: 2px;
  left: 2px;
  width: 11px;
  height: 11px;
  background: #fff;
  border-radius: 50%;
  transition: transform 0.15s;
}

.settings-toggle input:checked + .settings-toggle-track .settings-toggle-thumb {
  transform: translateX(13px);
}

.settings-toggle-label {
  font-size: 10px;
  font-weight: 700;
  letter-spacing: 0.12em;
  color: var(--color-muted);
  min-width: 18px;
}

.settings-row {
  display: flex;
  align-items: center;
  gap: 10px;
}

.settings-slider {
  -webkit-appearance: none;
  appearance: none;
  flex: 1;
  height: 12px;
  background: transparent;
  cursor: pointer;
}

.settings-slider::-webkit-slider-runnable-track {
  height: 3px;
  background: var(--color-border);
  border-radius: 2px;
}

.settings-slider::-webkit-slider-thumb {
  -webkit-appearance: none;
  appearance: none;
  width: 10px;
  height: 16px;
  background: var(--color-text);
  border-radius: 2px;
  margin-top: -6.5px;
}

.settings-value {
  font-size: 10px;
  font-variant-numeric: tabular-nums;
  color: var(--color-muted);
  min-width: 28px;
  text-align: right;
}

.settings-chip {
  background: transparent;
  border: 1px solid var(--color-border);
  color: var(--color-muted);
  font-family: var(--font);
  font-size: 10px;
  letter-spacing: 0.06em;
  padding: 3px 7px;
  border-radius: 3px;
  cursor: pointer;
  transition:
    border-color 0.1s,
    color 0.1s,
    background 0.1s;
}

.settings-chip:hover {
  border-color: var(--color-text);
  color: var(--color-text);
}

.settings-chip--active {
  border-color: var(--color-text);
  color: var(--color-text);
  background: color-mix(in srgb, var(--color-text) 8%, transparent);
}

.settings-range-label {
  font-size: 10px;
  color: var(--color-muted);
  letter-spacing: 0.08em;
}

.settings-number {
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

.settings-number:focus {
  outline: none;
  border-color: var(--color-text);
}

.settings-decks {
  display: flex;
  gap: 16px;
}

.settings-deck {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 8px;
  flex: 1;
}

.settings-deck-label {
  font-size: 9px;
  font-weight: 700;
  letter-spacing: 0.2em;
}

.settings-deck-grid {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 4px;
  width: 100%;
}

.settings-btn {
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

.settings-btn:hover {
  border-color: color-mix(in srgb, var(--accent) 60%, transparent);
  background: color-mix(in srgb, var(--accent) 6%, transparent);
}

.settings-btn--active {
  border-color: var(--accent);
  color: var(--accent);
  background: color-mix(in srgb, var(--accent) 10%, transparent);
  animation: btn-pulse 0.75s ease-in-out infinite;
}

.settings-btn--conflict {
  border-color: #e55;
  color: #e55;
  background: color-mix(in srgb, #e55 10%, transparent);
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

.settings-btn-key {
  font-size: 9px;
  font-weight: 700;
  letter-spacing: 0.15em;
  text-transform: uppercase;
  color: var(--accent);
  line-height: 1;
}

.settings-btn--active .settings-btn-key {
  letter-spacing: 0.25em;
}

.settings-btn-action {
  font-size: 12px;
  font-weight: 600;
  color: var(--color-text);
  line-height: 1;
}

.settings-footer {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  min-height: 22px;
}

.settings-error {
  font-size: 10px;
  color: #e55;
  flex: 1;
}

.settings-capture-hint {
  font-size: 10px;
  color: var(--color-muted);
  opacity: 0.65;
  flex: 1;
}

.settings-reset-btn {
  background: transparent;
  border: 1px solid var(--color-border);
  color: var(--color-muted);
  font-family: var(--font);
  font-size: 10px;
  letter-spacing: 0.08em;
  padding: 3px 8px;
  border-radius: 3px;
  cursor: pointer;
  flex-shrink: 0;
  transition:
    border-color 0.1s,
    color 0.1s;
}

.settings-reset-btn:hover {
  border-color: var(--color-text);
  color: var(--color-text);
}

.settings-close:focus-visible,
.settings-reset-btn:focus-visible,
.settings-chip:focus-visible {
  outline: 2px solid var(--color-text);
  outline-offset: 2px;
}

.settings-btn:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: 2px;
}

.settings-toggle input:focus-visible + .settings-toggle-track {
  outline: 2px solid var(--color-text);
  outline-offset: 2px;
}

.settings-slider:focus-visible {
  outline: 2px solid var(--color-text);
  outline-offset: 3px;
}
</style>
