<template>
  <div class="mapping-overlay" @click.self="close">
    <div
      ref="panelEl"
      class="mapping-panel"
      role="dialog"
      aria-modal="true"
      :aria-label="$t('settings.mapping.title')"
    >
      <div class="mapping-header">
        <span class="mapping-title">{{ $t('settings.mapping.title') }}</span>
        <span class="mapping-port">{{ port }}</span>
        <button class="mapping-close" v-tooltip="$t('settings.close')" @click="close">✕</button>
      </div>

      <div class="mapping-body">
        <p class="mapping-hint">{{ $t('settings.mapping.hint') }}</p>

        <section v-if="globalActions.length > 0" class="mapping-section">
          <div class="mapping-section-label">{{ $t('settings.mapping.global') }}</div>
          <div class="mapping-grid">
            <button
              v-for="action in globalActions"
              :key="slotKey(actionSlot(action, null))"
              class="mapping-slot"
              :class="slotClass(actionSlot(action, null))"
              @click="arm(actionSlot(action, null))"
              @dblclick.prevent="clear(actionSlot(action, null))"
            >
              <ControlIcon :shape="actionShape(action)" />
              <span class="mapping-slot-text">
                <span class="mapping-slot-action">{{ actionLabel(action) }}</span>
                <span class="mapping-slot-source">{{ sourceLabel(actionSlot(action, null)) }}</span>
                <span
                  v-if="buttonOf(actionSlot(action, null))"
                  class="mapping-slot-button"
                  :class="{
                    'mapping-slot-button--bad': isUnreleasable(action, actionSlot(action, null))
                  }"
                  @click.stop="cycleButton(actionSlot(action, null))"
                  >{{ $t(`settings.mapping.button.${buttonOf(actionSlot(action, null))}`) }}</span
                >
              </span>
            </button>
          </div>
        </section>

        <section class="mapping-section">
          <div class="mapping-section-head">
            <div class="mapping-section-label">{{ $t('settings.mapping.deck') }}</div>
            <div class="mapping-deck-tabs">
              <button
                v-for="deckId in DECKS_DISPOSITION"
                :key="deckId"
                class="btn-secondary mapping-chip"
                :class="{ 'mapping-chip--active': selectedDeck === deckId }"
                :style="{ '--accent': accent(deckId) }"
                @click="selectDeck(deckId)"
              >
                {{ deckId }}
              </button>
            </div>
          </div>

          <div class="mapping-grid">
            <button
              v-for="action in deckActions"
              :key="slotKey(actionSlot(action, boundDeck))"
              class="mapping-slot"
              :class="slotClass(actionSlot(action, boundDeck))"
              @click="arm(actionSlot(action, boundDeck))"
              @dblclick.prevent="clear(actionSlot(action, boundDeck))"
            >
              <ControlIcon :shape="actionShape(action)" />
              <span class="mapping-slot-text">
                <span class="mapping-slot-action">{{ actionLabel(action) }}</span>
                <span class="mapping-slot-source">{{
                  sourceLabel(actionSlot(action, boundDeck))
                }}</span>
                <span
                  v-if="buttonOf(actionSlot(action, boundDeck))"
                  class="mapping-slot-button"
                  :class="{
                    'mapping-slot-button--bad': isUnreleasable(
                      action,
                      actionSlot(action, boundDeck)
                    )
                  }"
                  @click.stop="cycleButton(actionSlot(action, boundDeck))"
                  >{{
                    $t(`settings.mapping.button.${buttonOf(actionSlot(action, boundDeck))}`)
                  }}</span
                >
              </span>
            </button>
          </div>
        </section>

        <section v-if="hasMixer" class="mapping-section">
          <div class="mapping-section-label">{{ $t('settings.mapping.mixer') }}</div>
          <div class="mapping-grid">
            <button
              v-for="action in mixerActions"
              :key="slotKey(actionSlot(action, boundDeck))"
              class="mapping-slot"
              :class="slotClass(actionSlot(action, boundDeck))"
              @click="arm(actionSlot(action, boundDeck))"
              @dblclick.prevent="clear(actionSlot(action, boundDeck))"
            >
              <ControlIcon :shape="actionShape(action)" />
              <span class="mapping-slot-text">
                <span class="mapping-slot-action">{{ actionLabel(action) }}</span>
                <span class="mapping-slot-source">{{
                  sourceLabel(actionSlot(action, boundDeck))
                }}</span>
                <span
                  v-if="buttonOf(actionSlot(action, boundDeck))"
                  class="mapping-slot-button"
                  :class="{
                    'mapping-slot-button--bad': isUnreleasable(
                      action,
                      actionSlot(action, boundDeck)
                    )
                  }"
                  @click.stop="cycleButton(actionSlot(action, boundDeck))"
                  >{{
                    $t(`settings.mapping.button.${buttonOf(actionSlot(action, boundDeck))}`)
                  }}</span
                >
              </span>
            </button>
            <button
              v-for="address in deckParams"
              :key="`${address.slot}/${address.param}`"
              class="mapping-slot"
              :class="slotClass(paramSlot(boundDeck, address))"
              @click="arm(paramSlot(boundDeck, address))"
              @dblclick.prevent="clear(paramSlot(boundDeck, address))"
            >
              <ControlIcon :shape="paramShape(address)" />
              <span class="mapping-slot-text">
                <span class="mapping-slot-action">{{ paramLabel(address) }}</span>
                <span class="mapping-slot-source">{{
                  sourceLabel(paramSlot(boundDeck, address))
                }}</span>
              </span>
            </button>
          </div>
        </section>

        <section class="mapping-section">
          <div class="mapping-section-label">{{ $t('settings.mapping.file') }}</div>
          <p class="mapping-hint">{{ $t('settings.mapping.folder') }}</p>
          <pre class="mapping-file">{{ fileText }}</pre>
        </section>
      </div>

      <div class="mapping-footer">
        <span v-if="midi.error" class="mapping-error">{{ midi.error }}</span>
        <span v-else-if="collisions > 0" class="mapping-error">{{
          $t('settings.mapping.collision', { count: collisions })
        }}</span>
        <span v-else-if="midi.savedPath" class="mapping-capture-hint">{{
          $t('settings.mapping.saved', { path: midi.savedPath })
        }}</span>
        <span v-else-if="armedKind" class="mapping-capture-hint">{{
          $t(`settings.mapping.move.${armedKind}`)
        }}</span>
        <span v-else class="mapping-capture-hint" style="opacity: 0" aria-hidden="true">·</span>
        <div class="mapping-actions">
          <button class="btn-secondary mapping-chip" @click="midi.resetDraft(port)">
            {{ $t('settings.mapping.reset') }}
          </button>
          <button class="btn-secondary mapping-chip" @click="copyFile">
            {{ copied ? $t('settings.mapping.copied') : $t('settings.mapping.copy') }}
          </button>
          <button class="btn-secondary mapping-chip" @click="midi.saveMapping(port)">
            {{ $t('settings.mapping.save') }}
          </button>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from 'vue';
import { useI18n } from 'vue-i18n';
import { markModalClosed, markModalOpen } from '@renderer/utils/modalStack';
import { trapTabWithin } from '@renderer/utils/focusTrap';
import ControlIcon from '@renderer/components/ControlIcon.vue';
import {
  actionShape,
  laidOut,
  paramShape,
  sectionOf,
  type MappingSection
} from '@renderer/utils/midiControlShape';
import { useMidiStore } from '@renderer/stores/midi';
import { useDecksStore, DECKS_DISPOSITION } from '@renderer/stores/decks';
import type { DeckId } from '@renderer/utils/types';
import {
  actionFor,
  actionSlot,
  bindingsBySlot,
  BUTTON_SPECS,
  capturesLabel,
  captureLabel,
  collidingKeys,
  unreleasable,
  mappingFileText,
  paramSlot,
  slotKey,
  stepsSuffix,
  type ActionSpec,
  type ButtonSpec,
  type MappingSlot,
  type ParamAddress
} from '@renderer/utils/midiMapping';

const { port } = defineProps<{ port: string }>();
const emit = defineEmits<{ close: [] }>();

const { t, te } = useI18n();
const midi = useMidiStore();
const decks = useDecksStore();

const panelEl = ref<HTMLElement | null>(null);
const selectedDeck = ref<DeckId>('A');
const copied = ref(false);
let copiedTimer: ReturnType<typeof setTimeout> | null = null;

// Always a deck, because the panel maps one deck at a time whatever the surface is.
// Whether the file names its decks or plays whichever it is given is read from how
// many decks were bound, and is nobody's to answer.
const boundDeck = computed((): string => selectedDeck.value);

// Laid out by this side: the vocabulary's order is the file's emit order, not a layout.
function inSection(section: MappingSection): ActionSpec[] {
  return laidOut(
    (midi.vocabulary?.actions ?? []).filter(
      (action) => !action.addressed && sectionOf(action) === section
    )
  );
}

const globalActions = computed(() => inSection('global'));
const deckActions = computed(() => inSection('deck'));
const mixerActions = computed(() => inSection('mixer'));

const deckParams = computed((): ParamAddress[] => midi.vocabulary?.deckParams ?? []);
const hasMixer = computed(() => deckParams.value.length > 0 || mixerActions.value.length > 0);

const armedKind = computed(() => actionFor(midi.vocabulary, midi.armed)?.kind ?? null);
const bound = computed(() => bindingsBySlot(midi.draft));
const colliding = computed(() => collidingKeys(midi.draft));
const collisions = computed(() => colliding.value.size);
const fileText = computed(() => mappingFileText(midi.draft));

function accent(deckId: DeckId): string {
  return decks.decks[deckId]?.accent ?? '#ffffff';
}

function labelled(key: string, fallback: string): string {
  return te(key) ? t(key) : fallback;
}

function actionLabel(action: ActionSpec): string {
  const name = labelled(`settings.mapping.action.${action.id}`, action.id);
  return `${name}${stepsSuffix(action.steps)}`;
}

function paramLabel(address: ParamAddress): string {
  return labelled(
    `settings.mapping.param.${address.slot}.${address.param}`,
    `${address.slot} ${address.param}`
  );
}

function resolutionLabel(resolution: string): string {
  return labelled(`settings.mapping.resolution.${resolution}`, resolution);
}

function isArmed(slot: MappingSlot): boolean {
  return midi.armed !== null && slotKey(midi.armed) === slotKey(slot);
}

function slotClass(slot: MappingSlot): Record<string, boolean> {
  return {
    'mapping-slot--armed': isArmed(slot),
    'mapping-slot--bound': bound.value.has(slotKey(slot)),
    'mapping-slot--colliding': colliding.value.has(slotKey(slot))
  };
}

// While a slot is armed it shows what the control currently reads as, so a fader
// resolving to 14-bit is visible before the capture settles.
function sourceLabel(slot: MappingSlot): string {
  if (isArmed(slot)) {
    const held = midi.pending;
    if (held === null || slotKey(held.slot) !== slotKey(slot)) return '···';
    return captureLabel(held.capture, resolutionLabel);
  }
  const captures = bound.value.get(slotKey(slot));
  return captures === undefined ? '—' : capturesLabel(captures, resolutionLabel);
}

// Only a button carries one, so a fader's row shows nothing here.
function buttonOf(slot: MappingSlot): ButtonSpec | null {
  return bound.value.get(slotKey(slot))?.[0]?.button ?? null;
}

function isUnreleasable(action: ActionSpec, slot: MappingSlot): boolean {
  return unreleasable(action, bound.value.get(slotKey(slot)));
}

async function cycleButton(slot: MappingSlot): Promise<void> {
  const held = buttonOf(slot);
  if (held === null) return;
  const next = BUTTON_SPECS[(BUTTON_SPECS.indexOf(held) + 1) % BUTTON_SPECS.length];
  await midi.setSlotButton(port, slot, next);
}

async function arm(slot: MappingSlot): Promise<void> {
  if (isArmed(slot)) {
    await midi.disarmSlot(port);
    return;
  }
  await midi.armSlot(port, slot);
}

async function clear(slot: MappingSlot): Promise<void> {
  await midi.clearSlot(port, slot);
}

async function selectDeck(deckId: DeckId): Promise<void> {
  if (midi.armed !== null) await midi.disarmSlot(port);
  selectedDeck.value = deckId;
}

async function copyFile(): Promise<void> {
  await navigator.clipboard.writeText(fileText.value);
  copied.value = true;
  if (copiedTimer) clearTimeout(copiedTimer);
  copiedTimer = setTimeout(() => {
    copied.value = false;
    copiedTimer = null;
  }, 1500);
}

function close(): void {
  emit('close');
}

async function onKeydown(nativeEvent: KeyboardEvent): Promise<void> {
  if (nativeEvent.key === 'Tab') {
    trapTabWithin(nativeEvent, panelEl.value);
    return;
  }
  if (nativeEvent.key !== 'Escape') return;
  nativeEvent.preventDefault();
  nativeEvent.stopImmediatePropagation();
  if (midi.armed !== null) {
    await midi.disarmSlot(port);
    return;
  }
  close();
}

function handleKeydown(nativeEvent: KeyboardEvent): void {
  onKeydown(nativeEvent).catch(() => {
    // `disarmSlot` swallows its own failure; nothing here can recover from one.
  });
}

onMounted(async () => {
  markModalOpen();
  window.addEventListener('keydown', handleKeydown, { capture: true });
  await midi.startLearn(port);
});

onUnmounted(() => {
  markModalClosed();
  window.removeEventListener('keydown', handleKeydown, { capture: true });
  if (copiedTimer) clearTimeout(copiedTimer);
  midi.stopLearn().catch(() => {
    // The panel is gone; a failed stop leaves the device in learn until the next open.
  });
});
</script>

<style scoped>
.mapping-overlay {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.7);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 1100;
}

.mapping-panel {
  background: var(--color-bg);
  border: 1px solid var(--color-border);
  border-radius: 6px;
  width: 680px;
  max-width: calc(100vw - 32px);
  max-height: calc(100vh - 64px);
  display: flex;
  flex-direction: column;
}

.mapping-header {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 12px 16px;
  border-bottom: 1px solid var(--color-border);
}

.mapping-title {
  font-size: 0.85rem;
  font-weight: 700;
  letter-spacing: 0.04em;
  color: var(--color-text);
}

.mapping-port {
  flex: 1;
  font-size: 0.7rem;
  color: var(--color-muted);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.mapping-close {
  background: none;
  border: none;
  color: var(--color-muted);
  cursor: pointer;
  font-size: 0.8rem;
  padding: 2px 6px;
}

.mapping-close:hover {
  color: var(--color-text);
}

.mapping-body {
  overflow-y: auto;
  padding: 16px;
  display: flex;
  flex-direction: column;
  gap: 20px;
}

.mapping-hint {
  margin: 0;
  font-size: 0.7rem;
  line-height: 1.5;
  color: var(--color-muted);
}

.mapping-section {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.mapping-section-head {
  display: flex;
  align-items: center;
  gap: 12px;
}

.mapping-section-label {
  font-size: 0.7rem;
  font-weight: 700;
  letter-spacing: 0.08em;
  text-transform: uppercase;
  color: var(--color-muted);
}

.mapping-deck-tabs,
.mapping-row {
  display: flex;
  gap: 4px;
}

.mapping-fields {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 8px;
}

.mapping-field {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.mapping-field-label {
  font-size: 0.65rem;
  color: var(--color-muted);
}

.mapping-input {
  background: var(--color-surface);
  border: 1px solid var(--color-border);
  border-radius: 3px;
  color: var(--color-text);
  font-size: 0.7rem;
  padding: 5px 7px;
}

.mapping-input:focus {
  border-color: var(--color-text);
  outline: none;
}

.mapping-assigned {
  font-size: 0.7rem;
  color: var(--color-muted);
}

.mapping-chip {
  font-size: 0.7rem;
  padding: 3px 10px;
  border-radius: 3px;
  cursor: pointer;
}

.mapping-chip--active {
  border-color: var(--accent, var(--color-text));
  color: var(--accent, var(--color-text));
}

.mapping-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(148px, 1fr));
  gap: 6px;
}

.mapping-slot {
  display: flex;
  align-items: center;
  gap: 9px;
  padding: 7px 9px;
  background: var(--color-surface);
  border: 1px solid var(--color-border);
  border-radius: 3px;
  color: var(--color-muted);
  cursor: pointer;
  text-align: left;
}

.mapping-slot:hover {
  border-color: var(--color-text);
}

.mapping-slot--bound {
  color: var(--color-text);
}

.mapping-slot--armed {
  border-color: var(--color-cue);
  color: var(--color-text);
}

.mapping-slot--colliding {
  border-color: var(--color-danger);
  color: var(--color-danger);
}

.mapping-actions {
  display: flex;
  gap: 6px;
}

.mapping-slot-text {
  display: flex;
  flex-direction: column;
  gap: 2px;
  align-items: flex-start;
  min-width: 0;
}

.mapping-slot-action {
  font-size: 0.7rem;
  font-weight: 600;
}

.mapping-slot-button {
  align-self: flex-start;
  margin-top: 1px;
  font-size: 0.6rem;
  letter-spacing: 0.04em;
  text-transform: uppercase;
  white-space: nowrap;
  padding: 1px 5px;
  border: 1px solid var(--color-border);
  border-radius: 2px;
  color: var(--color-muted);
}

.mapping-slot-button:hover {
  border-color: var(--color-text);
  color: var(--color-text);
}

.mapping-slot-button--bad {
  border-color: var(--color-danger);
  color: var(--color-danger);
}

.mapping-slot-source {
  font-size: 0.65rem;
  font-variant-numeric: tabular-nums;
  color: var(--color-muted);
}

.mapping-file {
  margin: 0;
  max-height: 180px;
  overflow: auto;
  padding: 10px;
  background: var(--color-surface);
  border: 1px solid var(--color-border);
  border-radius: 3px;
  font-size: 0.65rem;
  line-height: 1.45;
  color: var(--color-muted);
}

.mapping-footer {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding: 12px 16px;
  border-top: 1px solid var(--color-border);
}

.mapping-capture-hint,
.mapping-error {
  font-size: 0.7rem;
}

.mapping-capture-hint {
  color: var(--color-muted);
}

.mapping-error {
  color: var(--color-danger);
}
</style>
